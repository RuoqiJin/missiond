;; ══════════════════════════════════════════════════════
;; MissionD — Memory Pillar (Full Spec)
;; Parent:  v2/intent.lisp :: pillar memory
;; Status:  草稿 v0.3 — 3 成熟模块(自治 in/core/out) + 平铺未成熟区 + 横切
;; Created: 2026-04-19
;; ══════════════════════════════════════════════════════
;;
;; 结构一眼图:
;;
;;   ┌── 成熟模块 (各自内部 入/核心/出) ──────────────────┐
;;   │  module project-management  作用域 + 聚合              │
;;   │  module board                任务队列 + FSM + 派发     │
;;   └──────────────────────────────────────────────────────┘
;;   ┌── 未模块化 (平铺, 待晋升) ─────────────────────────┐
;;   │  category project-specs     Lisp 规约 + 加载器      │
;;   │  category system-support    底层表层 (含 writer 注记) │
;;   └──────────────────────────────────────────────────────┘
;;   ┌── 横切 ────────────────────────────────────────────┐
;;   │  db-trait / retention / migrations                     │
;;   │  (embedding + gen-crud 已迁出到 pillar 二 worker)      │
;;   └──────────────────────────────────────────────────────┘
;;
;; ══════════════════════════════════════════════════════

(intent memory
  (version "draft-v0.3")
  (parent "v2/intent.lisp :: pillar memory")
  (created "2026-04-19")
  (history
    (v0.1 "2026-04-19 — 4 分类扁平结构")
    (v0.2 "2026-04-19 — 仿 event-bus 三段式 (ingress/core/egress)")
    (v0.3   "2026-04-19 — 3 成熟模块自治 + 平铺未成熟区 + 横切; embedding/gen-crud 迁出")
    (v0.3.1 "2026-04-19 — search-engines 模块迁出到 pillar 二 worker (搜索是计算不是数据)"))
  (status "草稿 — 可演进; maturity-ladder 体现演进路径")

  (purpose "系统长期记忆: 2 成熟模块自治 + 底层数据层共享 + 横切基础设施")
  (storage "PostgreSQL via sqlx::PgPool")
  (gateway "crates/missiond-core/src/db/ — 唯一 DB 入口")

  (migrated-out-to-worker-pillar
    "embedding-provider (qwen3 via SonnetGateway) → pillar 二 2.2 sonnet-gateway + 2.3 embedding-worker"
    "gen-crud (Forge 冲压 CRUD) → pillar 二 2.5 code-generation"
    "search-engines (HNSW/FTS/trigram/tag + 融合打分) → pillar 二 2.6 search-engines (搜索是计算, 不是数据)")


  ;; ═════════════════════════════════════════════════════════════
  ;;  Scoping Index — 全系统表的项目化归类 (诚实摆盘)
  ;; ═════════════════════════════════════════════════════════════
  (scoping-index
    (desc "按 project_id 列实际存在与否归类")
    (authoritative-source "migrations/20260410100000_project_id_columns.sql")

    (group scoped-primary
      (rationale "服务 module project-management 两大目标")
      (tables
        (projects  :role "registry — project_id 就是主键")
        (knowledge :role "goal-2: KB 省 token" :has-column true)))

    (group scoped-secondary
      (rationale "有 project_id 列, 但不是 core 诉求 — 顺带覆盖")
      (tables
        (conversations :has-column true)
        (board_tasks   :has-column true)))

    (group inherited-scope
      (rationale "自身无 project_id, 通过 parent 表的外键推断")
      (from-conversations-session-id
        :count 11
        :tables "conversation_messages / turns / events / tool_calls / topic_vectors / labels; message_narrations / narration_cursors / embeddings / translations / labels")
      (from-knowledge-id
        :count 3
        :tables "knowledge_edges / kb_ast_links / kb_operation_queue")
      (from-board-tasks-id
        :count 1
        :tables "board_task_notes"))

    (group global-infrastructure
      (rationale "系统级或基础设施, 本就不应按项目分")
      :examples "system_timeline / daemon_state / credentials / event_log / blob_storage / inbox / events / tasks / dynamic_slots / slot_sessions / gemini_file_uploads / watcher_cursors / consumer_watermarks / reconcile_watermarks / gemini_cli_watermarks / backfill_*"
      :see "下方 table-catalog :: domain infrastructure / event-bus / slots-tasks")

    (group candidates-for-promotion
      (rationale "⚠ 当前全局无 project_id, 按项目分可能有价值 — 待决策")
      (candidates
        (retrospective_results :benefit "项目级复盘归档")
        (token_usage_ledger    :benefit "项目级成本追踪 — 最想看的指标")
        (prompt_snapshots      :benefit "项目级 prompt 调优对比")
        (incidents             :benefit "项目级告警分级")
        (gemini_requests       :benefit "项目级 LLM 使用模式")
        (agent_questions       :benefit "项目级 agent 问题集")
        (slot_tasks            :benefit "项目级 slot 任务记录")
        (user_intents          :benefit "项目级意图识别")
        (intent plan workflow  :benefit "新 specs 三表, 应加 project_id (20260420 migration 未加)")
        (ast_nodes ast_file_meta beacons beacon_nodes :benefit "项目级代码索引")
        (skill_topics skill_blocks skill_versions skill_executions :benefit "项目私有技能库")
        (image_descriptions    :benefit "项目级图片注释")
        (router_chat_archive   :benefit "项目级 router 聊天归档")
        (flows-yaml-files      :benefit "项目私有 flow 模板" :kind "文件, 非 DB"))))


  ;; ═════════════════════════════════════════════════════════════
  ;;  Table Catalog — 61 张 PG 表实况
  ;; ═════════════════════════════════════════════════════════════
  (table-catalog
    (desc "migrations/*.sql 的 CREATE TABLE 实际统计 — 按业务域分 12 组")
    (total 61)
    (source "crates/missiond-core/migrations/")

    (domain project (count 1)
      (projects :purpose "项目注册 — id/path/intent_path/active/slots/github_url/vault"))

    (domain knowledge-kb (count 7)
      (knowledge           :purpose "语义级记忆 40+ category (architecture/bugfix/policy/...)" :scoping primary)
      (knowledge_edges     :purpose "KB 条目间关系图" :scoping inherited)
      (kb_access_log       :purpose "KB 访问审计 — 谁读了什么")
      (kb_operation_queue  :purpose "KB 变更异步队列")
      (kb_ast_links        :purpose "KB 条目 ↔ AST 节点关联")
      (prompt_snapshots    :purpose "模型输入输出存档 — prompt 迭代用")
      (image_descriptions  :purpose "图片的文本描述 (vision)"))

    (domain conversations (count 13)
      (conversations              :purpose "会话主表 — session_id / summary / project_id" :scoping secondary)
      (conversation_messages      :purpose "消息原始记录 (PTY JSONL 来源)")
      (conversation_turns         :purpose "turn 级切分 (user ↔ assistant)")
      (conversation_events        :purpose "会话事件流 (tool use / status change)")
      (conversation_tool_calls    :purpose "工具调用详情")
      (conversation_topic_vectors :purpose "话题向量 — 语义聚类")
      (conversation_labels        :purpose "会话级打标 (主题/质量)")
      (message_embeddings         :purpose "消息级向量 (去重)")
      (message_embedding_skips    :purpose "跳过 embedding 的记录")
      (message_narrations         :purpose "消息 LLM 摘要 (briefing-worker 产)")
      (narration_cursors          :purpose "narration 游标 (防重)")
      (message_translations       :purpose "消息翻译 (多语种)")
      (message_labels             :purpose "消息级打标"))

    (domain board (count 3)
      (board_tasks      :purpose "任务队列 — 27 列, 7 态 FSM" :scoping secondary)
      (board_task_notes :purpose "任务附注" :scoping inherited)
      (agent_questions  :purpose "Agent 卡住时提的问题"))

    (domain slots-tasks (count 4)
      (tasks          :purpose "任务表 (老版, 疑似 deprecated)")
      (slot_sessions  :purpose "槽位会话 (CLI 进程生命周期)")
      (slot_tasks     :purpose "槽位任务队列 (slot 专属 lifecycle)")
      (dynamic_slots  :purpose "按需创建的动态槽位"))

    (domain skills (count 4)
      (skill_topics     :purpose "技能主题 (顶层分类)")
      (skill_blocks     :purpose "技能内容块")
      (skill_versions   :purpose "技能版本")
      (skill_executions :purpose "技能执行记录"))

    (domain specs (count 4)
      (intent       :purpose "项目 intent.lisp 的 DB 镜像 (新表 20260420)")
      (plan         :purpose "执行计划 (新表 20260420)")
      (workflow     :purpose "工作流模板 (新表 20260420)")
      (user_intents :purpose "用户意图识别记录"))

    (domain ast-indexing (count 4)
      (ast_nodes     :purpose "AST 节点 — 文件/函数/类结构化")
      (ast_file_meta :purpose "文件级元数据 (hash/size/modified)")
      (beacons       :purpose "代码 beacon — 关注点标记")
      (beacon_nodes  :purpose "beacon 关联的 AST 节点"))

    (domain observability (count 5)
      (system_timeline     :purpose "统一时间线 — 7 天 TTL" :scoping global)
      (incidents           :purpose "告警/异常聚合" :scoping candidate)
      (token_usage_ledger  :purpose "LLM 调用成本追踪" :scoping candidate)
      (gemini_requests     :purpose "Gemini API 调用日志" :scoping candidate)
      (gemini_file_uploads :purpose "Gemini 文件上传缓存"))

    (domain audit-products (count 2)
      (retrospective_results :purpose "会话复盘 JSON" :scoping candidate)
      (router_chat_archive   :purpose "Router 聊天归档" :scoping candidate))

    (domain event-bus
      :count-in-memory 0
      :owned-by "pillar 四 event-bus :: §4.6 persistence-layer"
      :tables-elsewhere "event_log / event_subscriptions / blob_storage / dead_letter_queue (4 张, 归 pillar 四 管)"
      :migrated-out "2026-04-19 v1.2.0 — 所有权从此转给 pillar 四 (它们是 event-bus 的实现细节)"
      :note "其他 pillar 不直接读写这 4 张表, 必须通过 pillar 四 的 (append) / (subscribe) API")

    (domain infrastructure (count 10)
      (daemon_state          :purpose "daemon 级全局状态")
      (credentials           :purpose "凭据存储 (加密)")
      (inbox                 :purpose "收件箱 (老表)")
      (events                :purpose "事件表 (老版, 疑似 deprecated)")
      (reconcile_watermarks  :purpose "对账游标")
      (gemini_cli_watermarks :purpose "Gemini CLI 水位游标")
      (watcher_cursors       :purpose "观察者游标")
      (consumer_watermarks   :purpose "消费者水位")
      (backfill_progress     :purpose "回填进度追踪")
      (backfill_failures     :purpose "回填失败记录"))

    (legacy-note "⚠ tasks / inbox / events 疑似老版 schema 不再活跃, 待实地确认是否可删"))


  ;; ═════════════════════════════════════════════════════════════
  ;;  Non-DB Forms — 数据库之外的记忆载体 (6 种形式)
  ;; ═════════════════════════════════════════════════════════════
  (non-db-forms
    (desc "记忆不止 DB — 还有 Lisp 规约 / YAML 模板 / Markdown 手写 / 外部流 / 向量 / 大对象 6 种载体")

    ;; ── 形式 1: Lisp 声明文件 (self-describing specs) ──
    (form lisp-spec-files
      (desc "Lisp 自我描述文件 — 设计时产物, 可被 Agent 读取以省导航")

      (variant project-intent
        :path "<project>/.missiond/intent.lisp"
        :scope per-project
        :writer "pillar 二 2.3 lisp-survey-worker (sonnet)"
        :reader "mission_intent tool (MCP)"
        :purpose "每项目的架构画像, goal-1 的直接服务对象")

      (variant system-main
        :path ".missiond/v2/intent.lisp"
        :scope global
        :purpose "系统主架构 — 6 大 pillar 总览")

      (variant system-detail
        :path ".missiond/v2/intent-{event-bus,memory}.lisp"
        :scope global
        :purpose "pillar 级细节规格"
        :note "event-bus.lisp 是 frozen v1.1.0; memory.lisp (本文件) 是 v0.3.1 草稿")

      (variant workflow-templates
        :path ".missiond/workflows/*.lisp"
        :scope global
        :purpose "可复用方法论模板 (如 bus-refactor.lisp)"
        :reader "人工参考 + Agent 检索"))

    ;; ── 形式 2: YAML 流程模板 ──
    (form yaml-flow-templates
      (desc "flow-engine-v2 的声明式节点编排")
      :path "$MISSIOND_HOME/flows/*.yaml"
      :scope "全局共享 (候选: scoping-index :: candidates-for-promotion)"
      :loader "daemon/src/engine/flow/loader.rs"
      :executor "pillar 二 2.4 orchestration :: flow-engine-v2"
      :parser "serde_yaml::from_str::<FlowDefinition>")

    ;; ── 形式 3: Markdown + YAML 手写记忆 ──
    (form markdown-handwritten-memories
      (desc "人工手写的持久记忆 — Markdown + YAML frontmatter")

      (variant user-global-claudemd
        :path "~/.claude/CLAUDE.md"
        :scope global-user
        :purpose "全局用户指令 (跨项目永久适用, 每次会话都加载)"
        :loaded-by "Claude Code system prompt")

      (variant project-claudemd
        :path "<project>/CLAUDE.md"
        :scope per-project
        :purpose "项目级 Claude 指令 (随项目 git 版本化, 每次会话都加载)"
        :loaded-by "Claude Code system prompt")

      (variant auto-memory-vault
        :path "~/.claude/projects/{encoded-path}/memory/*.md"
        :index "~/.claude/projects/{encoded-path}/memory/MEMORY.md"
        :scope per-project-per-user
        :types "user / feedback / project / reference"
        :format "YAML frontmatter (name / type / description) + Markdown 正文"
        :purpose "Agent 跨会话持久记忆 — 反馈 / 习惯 / 项目背景"
        :see "module project-management :: helper project-memories-vault"))

    ;; ── 形式 4: 外部原始流 (记忆的原料, worker 消费入 DB) ──
    (form external-source-streams
      (desc "外部系统产生的原始流, MissionD 的 worker 消费并解包入 DB")

      (source pty-session-jsonl
        :path "~/.claude/projects/{encoded-path}/*.jsonl"
        :producer "Claude Code CLI (外部工具)"
        :consumer "pillar 二 2.3 conversation-logger (local)"
        :writes-to "conversations / conversation_messages / conversation_turns / conversation_events"
        :idempotency "(session_id, turn_id) 去重")

      (source git-commit-history
        :path "<project>/.git/"
        :producer "git (外部)"
        :consumer "lisp-survey-worker (通过 ContextualCommitDetected 事件)"
        :triggers "lisp-surveyor slot dispatch → 更新项目 intent.lisp"
        :debounce "60s per project_id"))

    ;; ── 形式 5: 二进制向量 (embedding) ──
    (form embedding-vectors
      (desc "pgvector 512-dim 二进制 — 存 DB 列但是特殊形式")
      :columns "knowledge.embedding_vec / conversations.summary_embedding / conversation_topic_vectors.vec / message_embeddings.vec"
      :dim 512
      :provider "pillar 二 2.2 sonnet-gateway (qwen3)"
      :generator "pillar 二 2.3 embedding-worker (sonnet)"
      :consumer "pillar 二 2.6 search-engines :: engine vector-hnsw"
      :invariant "禁止降级兜底, 失败直接报错")

    ;; ── 形式 6: Side-channel 大对象 ──
    (form side-channel-blobs
      (desc ">8KB 大对象 — claim-check 模式 side-channel 存储")

      (location blob-storage-pg
        :table "blob_storage"
        :purpose "event-bus 中 >8KB payload 的 side-channel 主存储"
        :threshold "8KB"
        :see "pillar 四 event-bus :: claim-check (section 4.2 decide)")

      (location gemini-file-remote
        :table "gemini_file_uploads (PG 只存引用)"
        :actual-storage "Gemini 服务端"
        :purpose "Gemini File API 的文件引用缓存")))
  ;;  项目是记忆的作用域单位 — 注册 / 路径解析 / 数据隔离 / 聚合
  ;; ═════════════════════════════════════════════════════════════
  (module project-management
    (desc "两大目标: ①MCP 精准召回项目 intent.lisp(省导航) ②项目级 KB 过滤(省 token)")
    (maturity "成熟")

    ;; ── 为什么存在 ──
    (primary-goals
      (goal-1 mcp-intent-precision
        :problem  "agent 调 MCP 不知道当前项目, 需多步 cwd→path→intent 搜索"
        :solution "ProjectRegistry::resolve(cwd) → project_id → projects.intent_path → 读文件"
        :benefit  "每次 MCP 调用省 3-5 步导航")
      (goal-2 kb-token-economy
        :problem  "KB 全局查召回无关项目条目, 干扰 LLM 判断 + 浪费 token"
        :solution "WHERE knowledge.project_id = $X OR IS NULL"
        :benefit  "LLM context 更聚焦, token 使用下降"))

    ;; ── 入点 ──
    (module-ingress
      (desc "项目注册 + 项目作用域传播的写入路径")

      (writer mcp-project-mutation
        :tools  "mission_project init / sync / set_active / vault_sync / import_universe / survey"
        :writes "projects"
        :code   "daemon/src/handlers/knowledge/project.rs"
        :kind   "MCP 同步")

      (writer project-memory-vault-edit
        :source "用户手动编辑 + Claude Code 文件操作"
        :writes "~/.claude/projects/{encoded}/memory/*.md (文件, YAML 前置元数据)"
        :kind   "文件系统, 不走 DB")

      (writer project-scope-propagation
        :desc   "其他模块的 writer 插入时自动打 project_id (作用域传播机制)"
        :writers-list ("conversation-logger → conversations.project_id"
                       "kb-mutation → knowledge.project_id"
                       "board-lifecycle → board_tasks.project_id"
                       "retro-worker → retrospective_results.project_id")
        :resolve-via "ProjectRegistry::resolve(cwd) 或 handler 显式参数"))

    ;; ── 逻辑核心: 主路径 + plumbing + 辅助 ──
    (module-core
      (desc "围绕两大 primary-goal 组织: path 直接服务 goal, plumbing 共用基础, helper 面板类")

      ;; ── 主路径 (每条直接对应一个 goal) ──
      (path project-intent-access
        :serves goal-1
        :flow  "cwd → ProjectRegistry::resolve → project_id → projects.intent_path → 读文件"
        :entry "mission_intent read (cwd 自动解析, 无需手动传 project)")

      (path project-kb-access
        :serves goal-2
        :rule  "WHERE knowledge.project_id = $X OR IS NULL"
        :entry "mission_kb_query / mission_kb_search (默认 project_scoped=true)"
        :batch "mission_kb_batch_set_project — 把现有条目归到项目")

      ;; ── plumbing (共用基础, 让主路径跑得起来) ──
      (plumbing project-registry
        (desc "两条 path 的共用基础 — projects 表 + 内存索引 + cwd 解析器")
        :tables "projects"
        :schema-cols "id(PK) / path(unique) / intent_path / active / slots[] / github_url / timestamps"
        :in-memory "启动时从 PG 加载到内存; longest-prefix 路径匹配; 运行期只读"
        :migration "20260410000000_projects.sql + 20260410200000 seed"
        :backfill "conversations.project_id 按 path 匹配回填"
        :resolver "ProjectRegistry::resolve(cwd) / exclusive_slots / list_active"
        :code "crates/missiond-core/src/{types,db/pg}/project.rs + daemon/src/state.rs (SharedProjectRegistry)")

      (plumbing scope-mechanism
        (desc "project_id 列 + 查询规则 — 所有带 project_id 的表共用的隔离工具")
        :rule "WHERE project_id = $X OR project_id IS NULL"
        :semantics "NULL = 全局/跨项目共享; 非 NULL = 项目私有"
        :applies-to "4 张表 (实况): projects / knowledge / conversations / board_tasks"
        :primary-beneficiaries "knowledge (goal-2) / projects (goal-1 基础)"
        :secondary-beneficiaries "conversations / board_tasks (有列但非核心诉求)"
        :honesty "仅此 4 张有 project_id 列; 其他 ~15 张通过 parent FK 继承, ~40 张全局无 scope"
        :see-also "顶层 scoping-index (按 scoping 归类) + table-catalog (按业务域归类)")

      ;; ── 辅助 (用户常用面板, 不是主路径) ──
      (helper project-context-aggregator
        (desc "mission_project context 视图 — 多源聚合 观察项目当前状态")
        :code "daemon/src/handlers/knowledge/project.rs :: build_*"
        :aggregates ("build_intent_summary: 读 intent.lisp → survey-date / pillar 列表"
                     "conversation_stats_by_project: COUNT / status 分布"
                     "kb_stats_by_project: BY category"
                     "slot-status: 项目关联的 compute_slots 状态")
        :priority "辅助 — 不是主路径, 但 mission_project context 常用")

      (helper project-memories-vault
        (desc "项目专属的人工手写记忆库 — 独立于 DB")
        :storage "~/.claude/projects/{encoded}/memory/*.md"
        :format "Markdown + YAML 前置元数据(name/type/description)"
        :code "daemon/src/handlers/knowledge/project_memory.rs"
        :invariant "path-traversal 防护; 独立于 DB scoping"
        :priority "辅助 — 文件系统级 per-project, 不走 DB"))

    ;; ── 出点 ──
    (module-egress
      (desc "项目级别的读取出口")

      (reader mcp-project-views
        :tools "mission_project get / list / context / memories"
        :reads "projects + ProjectRegistry 聚合 + project-memories-vault"
        :code  "daemon/src/handlers/knowledge/project.rs")

      (reader frontend-project-stream
        :emits "项目列表 / 活跃切换 / 项目元数据变更"
        :via   "pillar 六 6.2 process-transport :: ws-server")))


  ;; ═════════════════════════════════════════════════════════════
  ;;  成熟模块 2: 看板 (Board)
  ;;  任务队列 — worker 的唯一输入面 + DAG 调度中心
  ;; ═════════════════════════════════════════════════════════════
  (module board
    (desc "任务队列 — 所有可分发工作的权威存储, worker 的唯一输入面")
    (maturity "成熟 — 7 态 FSM + 8 MCP 工具 + autopilot 消费者齐备")
    :migrated-from "v1: pillar worker :: section board; v0.3 晋升为独立模块"

    ;; ── 入点 ──
    (module-ingress
      (desc "看板任务的写入路径 — MCP lifecycle + engine 内部推进")

      (writer mcp-board-lifecycle
        :tools  "mission_board_create / update / claim / decompose / retry / note_add / delete"
        :count  7
        :writes "board_tasks / board_task_notes"
        :code   "daemon/src/handlers/knowledge/board.rs")

      (writer autopilot-engine
        :code   "daemon/src/engine/intent_engine/autopilot.rs"
        :tick   "5-10s"
        :writes "board_tasks (CAS claim / status 推进 / lease 回收)"
        :tick-pipeline "memory-scheduler → extraction-check → board-task-dispatch → flow-progression → supervision-check"
        :invariant "open→running 是原子 CAS, 多 executor 并发安全")

      (writer flow-engine-v2
        :code   "daemon/src/engine/flow/{mod,runner,handlers,loader}.rs"
        :writes "board_tasks.flow_context (每节点执行后 persist_context)"
        :node-types 5 "LlmCall / SlotTask / McpTool / DaemonAction / ParallelSlotTasks"))

    ;; ── 逻辑核心 ──
    (module-core
      (desc "board_tasks 数据模型 + 7 态 FSM + 核心操作 + DomainEvent")

      (component data-model
        :table "board_tasks"
        :columns 27
        :grouping "身份 / 内容 / 生命周期 / 占用 / 层级 / 执行 / 流程 / 作用域 / 去重 / UI"
        :schema-cols "id / title / description / status(enum:7) / priority / category / engineering_phase / executor / assigned_slot / depends_on(JSONB) / dedupe_key / lease_until / retry_count / autopilot / flow_template / flow_phase / flow_context(JSON) / project_id / claim_executor_id / claim_executor_type / claimed_at / parent_id"
        :indexes 5
        :code "crates/missiond-core/src/db/board.rs")

      (component state-machine
        (desc "7 态有限状态机 + 原子性保障 + lease 恢复")
        :states (open running verifying done failed blocked skipped)
        :transitions
          ("open→running       : CAS claim_executor_id (原子)"
           "open→blocked       : check_dependencies 失败"
           "running→verifying  : engineering flow 执行完待审"
           "running→done/failed: executor 报完成 / lease 超时"
           "blocked→open       : 上游依赖解除"
           "terminal→open      : mission_board_retry (reset + retry_count++)")
        :atomicity "open→running 是 SQL CAS 原子操作"
        :recovery "lease_expires_at 超期 → recover_stale_running_tasks 强制 reset"
        :terminal-irreversibility "done/failed/skipped 不能前转, 只能 retry")

      (component core-operations
        (desc "8 个 lifecycle 操作, 都走 BoardStore trait")
        :operations (create claim update decompose retry note-add delete query)
        :code "crates/missiond-core/src/db/board.rs + daemon/src/handlers/knowledge/board.rs"
        :notes ("decompose: 派 slot 执行 AI 分析 → 生成子任务 DAG"
                "retry: 可选级联重置下游的 BFS 算法"
                "delete: CASCADE 删子任务 + notes"))

      (component events-emitted
        (desc "BoardEvent domain event — 骨干在 pillar 四 event-bus")
        :variants "BoardTaskCreated / StatusChanged / NoteAdded / Claimed / Deleted / Updated"
        :emitted-by "MCP handler + autopilot engine"
        :persisted-via "pillar 四 event_log → timeline-writer fanout → system_timeline"))

    ;; ── 出点 ──
    (module-egress
      (desc "看板的查询 + 前端实时推送 + autopilot 内部扫描")

      (reader mcp-board-query
        :tool    "mission_board_query"
        :actions "list / get / search / summary / clear_done"
        :reads   "board_tasks / board_task_notes")

      (reader frontend-board-stream
        :source "pillar 四 event_log subscribe BoardEvent topic"
        :emits  "BoardTask* events → 前端看板实时更新"
        :via    "daemon/src/bus/ws_bridge.rs")

      (reader autopilot-tick-scan
        :desc  "autopilot 做决策前的扫描(内部消费者)"
        :query "WHERE auto_execute=1 AND status='open' ORDER BY (assignee 存在) → order_idx"
        :note  "这是同一个 autopilot 既读又写, 读在前(决策), 写在后(占用)")))



  ;; ═════════════════════════════════════════════════════════════
  ;;  未模块化的平铺分类 — 待成熟后晋升为模块
  ;; ═════════════════════════════════════════════════════════════

  (category project-specs
    (desc "设计时 Lisp 规约文件 + 加载 / 感知 / 查询")
    (maturity "未成熟 — lisp-surveyor 仍在完善; 待稳定可晋升模块")

    (component intent-lisp-registry
      :storage "projects.intent_path 字段指向项目 .missiond/intent.lisp 文件")

    (component intent-lisp-loader
      :code "daemon/src/handlers/knowledge/intent.rs"
      :actions "read / section / summary / list"
      :caching "无缓存, 每次读文件(支持热 reload)"
      :reader "mission_intent MCP tool")

    (component intent-lisp-auto-survey
      :worker  "pillar 二 2.3 :: lisp-survey-worker (sonnet)"
      :trigger "ContextualCommitDetected 事件 → slot dispatch"
      :debounce "60s per project_id"
      :writes  "项目 intent.lisp 文件 (唯一走文件系统的 writer)"
      :self-loop-guard "slot_id == lisp-surveyor 的 commit 自动跳过")

    (component flow-definitions
      :storage "$MISSIOND_HOME/flows/*.yaml"
      :loader  "daemon/src/engine/flow/loader.rs"
      :executor "pillar 二 2.4 orchestration :: flow-engine-v2")

    (component prompt-snapshot-archive
      :table   "prompt_snapshots"
      :purpose "存档模型输入输出, 用于迭代学习 / prompt 工程"))


  (category system-support
    (desc "底层数据层 — 被所有模块使用, 无独立业务语义; 每张表标注 :writers / :readers")
    (maturity "基础层 — 结构稳定, 但包含未来可能晋升的内容(如 conversations)")

    (component conversation-tables
      :tables "conversations / conversation_messages / conversation_turns / message_narrations"
      :code "crates/missiond-core/src/db/conversation.rs"
      :writers ("pillar 二 2.3 :: conversation-logger(local): PTY JSONL 实时解包"
                "pillar 二 2.3 :: message-narrator(sonnet): LLM 摘要"
                "MCP: mission_conversation_reconcile / analyze")
      :readers ("MCP: mission_conversation_query / analyze(action=get)"
                "module project-management :: context-aggregator"
                "前端 WS: conversation-event-stream")
      :scoping "project_id — 见 module project-management :: scope-mechanism")

    (component knowledge-tables
      :tables "knowledge / knowledge_edges / prompt_snapshots"
      :code "crates/missiond-core/src/db/knowledge.rs"
      :writers ("MCP: mission_kb_mutate / mission_kb_remember"
                "pillar 二 2.3 sonnet 组: arch-maintenance / experience-harvester"
                "pillar 二 2.3 local 组: tagger-chunker"
                "module search-engines :: embedding-column-writes (embedding 列)")
      :readers "见 pillar 二 2.6 search-engines :: consumers (全部搜索消费者)"
      :scoping "project_id — 见 module project-management")

    (component audit-trail-tables
      :tables "tool_calls / conversation_events / retrospective_results"
      :code "crates/missiond-core/src/db/audit.rs"
      :writers ("MCP dispatch 中间件 → tool_calls (自动审计所有工具调用)"
                "pillar 二 2.3 :: retro-worker(sonnet) → retrospective_results"
                "pillar 二 2.3 :: conversation-logger → conversation_events")
      :readers ("MCP: mission_audit / mission_retrospective_manage / mission_llm_trace")
      :invariant "append-only; UPDATE 只改状态列不改身份列")

    (component timeline-table
      :table "system_timeline"
      :code "crates/missiond-core/src/db/timeline.rs"
      :columns "seq(autoincrement) / trace_id / span_id / parent_span_id / event_type / summary / payload / created_at"
      :writers "pillar 四 event-bus :: timeline-writer (订阅 event_log 所有 topic)"
      :readers ("MCP: mission_timeline"
                "前端 WS: timeline-event-stream")
      :queries "by trace_id / by event_type / 分层抽样 / FTS 搜索"
      :ttl "7 天自动清理 (retention_cron)")

    (component observability-tables
      (sub-table gemini-trace
        :tables "gemini_requests / gemini_file_cache"
        :writers "pillar 二 2.3 :: gemini-logger(local)"
        :readers "MCP: mission_llm_trace")

      (sub-table incidents
        :table "incidents"
        :writers "pillar 四 event-bus :: incident-writer (订阅 IncidentEvent)"
        :dedup "dedupe_key + 时间窗口")

      (sub-table token-usage
        :table "token_usage"
        :writers "pillar 四 event-bus :: token-usage-writer (订阅 LlmEvent.usage)"
        :aggregations "by model / slot / conversation / time-series")

      (sub-table narration-cursor
        :table "narration_cursors"
        :writers "pillar 二 2.3 :: message-narrator(sonnet)"
        :purpose "避免重复 narrate 同一批 message"))

    (component embedding-storage
      (desc "向量物理存储位置 — 生成路径和 provider 见 pillar 二 worker")
      :columns "knowledge.embedding_vec(512) / conversations.summary_embedding / conversation_topic_vectors"
      :see-generator "pillar 二 2.3 :: embedding-worker (sonnet) + 2.2 :: sonnet-gateway (qwen3 路由)")

    (component misc-tables
      (desc "其他 MCP 工具对应的小表 — 暂未抽模块")
      (sub-table skills
        :table   "skills"
        :writers "MCP: mission_skill_mutate"
        :readers "MCP: mission_skill_query / mission_skill_context")

      (sub-table permission
        :tables  "permission_policies / learned_permissions"
        :writers "MCP: mission_permission_mutate"
        :readers "MCP: mission_permission_query")

      (sub-table inbox
        :table   "inbox_messages"
        :writers "MCP: mission_inbox create"
        :readers "MCP: mission_inbox list/get")))


  ;; ═════════════════════════════════════════════════════════════
  ;;  横切能力 — 贯穿所有模块 + 分类
  ;;  (embedding-provider + gen-crud 已迁出到 pillar 二 worker)
  ;; ═════════════════════════════════════════════════════════════
  (cross-cutting
    (desc "不归属任何模块或分类, 贯穿全记忆 pillar 的基础能力")

    (capability db-trait-abstraction
      (desc "MissionStore 超 trait 聚合 13 个领域 store, 隔离 PG/SQLite 实现")
      :trait "MissionStore"
      :code "crates/missiond-core/src/db/traits.rs (~750 行)"
      :stores 13
      :store-list "Conversation / Message / ToolCall / Event / Retrospective / Vision / Knowledge / Board / Timeline / Slot / Skill / Observability / Project"
      :invariant "其他 crate 只依赖 trait 不依赖实现 — 可切换后端"
      (impl pg-store
        :desc "生产实现: 原生 async sqlx"
        :target "crates/missiond-core/src/db/pg_*/ (15 文件)")
      (impl sqlite-store
        :desc "遗留实现, spawn_blocking 包装"
        :status "⚠ deprecated — 仅保留 pg::migrate_from_sqlite 辅助"))

    (capability retention-policy
      (desc "按表粒度的保留/清理规则")
      :rules ("system_timeline:     7 天 TTL (retention_cron)"
              "conversation_*:       append-only, 无自动清理"
              "knowledge:            append-only, 按 access_count 手动归档"
              "incidents:            粗粒度按时间 DELETE"
              "tool_calls:           append-only")
      :code "daemon/src/bus/retention_cron.rs")

    (capability migrations-runner
      (desc "schema 演进管理 — daemon 启动 phase 1 自动跑")
      :code "crates/missiond-core/migrations/"
      :count "16 migrations (含 seed + backfill)"
      :automation "sqlx::migrate! 编译期检查 + 运行期执行"
      :entry "启动 phase 1"))


  ;; ═════════════════════════════════════════════════════════════
  ;;  跨分类注记 — 演进路径 + 开放问题
  ;; ═════════════════════════════════════════════════════════════
  (cross-cutting-notes

    (maturity-ladder
      (mature-modules   2 "project-management / board")
      (flat-categories  2 "project-specs / system-support")
      (cross-cutting    3 "db-trait / retention / migrations")
      (external-modules 1 "search-engines 已迁至 pillar 二 2.6 (搜索是计算)")
      (strategy "某个区块稳定后, 相关 writer/reader/core 抽成 module; 若本质是计算而非数据, 则归 worker pillar"))

    (migration-log
      "2026-04-19 v0.3:"
      "(1) embedding-provider 迁出到 pillar 二 2.2 sonnet-gateway (双角色: API + embedding)"
      "(2) gen-crud 迁出到 pillar 二 2.5 code-generation (新增 section)"
      "(3) 3 个成熟能力(project-management / board / search-engines) 升级为模块, 各自内部 in/core/out"
      "(4) scoping-model 从 cross-cutting 下沉到 module project-management :: scope-mechanism (因项目是 scoping 的所有者)"
      "2026-04-19 v0.3.1:"
      "(5) search-engines 模块整体迁出到 pillar 二 2.6 (搜索是 computation, 记忆是 data; 概念归属纠正)")

    (design-rationale
      "module = 已稳定, 有清晰的入口/核心/出口闭环, 值得单独导航;"
      "category = 底层数据层或未成熟规约, 结构稳定但使用模式还在扩展, 按表标注 writer/reader;"
      "cross-cutting = 贯穿全局的基础机制, 无独立身份")

    (pending-questions
      "Q1: scoping-model 下沉到 project-management 模块后, 是否仍需要在 cross-cutting 留一个指针?"
      "Q2: conversations 已经很稳定, 什么时候晋升成独立模块? (依赖: message-narrator 路径是否会变)"
      "Q3: audit-trail / timeline / observability 未来合并成一个"observability 模块"?"
      "Q4: project-specs 晋升需要什么条件? (lisp-surveyor 稳定 + plan.lisp/workflow.lisp 加载器收口)"))

) ;; end intent memory
