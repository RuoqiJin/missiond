;; ══════════════════════════════════════════════════════
;; MissionD — Memory Pillar (Full Spec)
;; Parent:  v2/intent.lisp :: pillar memory
;; Status:  草稿 v0.4 — 4 成熟模块各管自己的表 + 精简 system-support
;; Created: 2026-04-19
;; ══════════════════════════════════════════════════════
;;
;; v0.4 重大重构 — 按用户指令把表归到 4 个业务模块:
;;
;;   ┌── 4 成熟模块 (各自 in/core/out + 显式 owned-tables) ────────┐
;;   │  module project-management     9 张 (projects + specs + skills)│
;;   │  module board                  3 张 (board_tasks 系列)         │
;;   │  module kb-manager   NEW      10 张 (knowledge + ast 索引)     │
;;   │  module conversation-logs NEW 15 张 (claude/gemini/codex 会话) │
;;   └──────────────────────────────────────────────────────────────┘
;;   ┌── 分类 ────────────────────────────────────────────────────┐
;;   │  category system-support    ~20 张 (观测 / 基建 / 运行时游标)  │
;;   └──────────────────────────────────────────────────────────────┘
;;   ┌── 横切 ────────────────────────────────────────────────────┐
;;   │  db-trait / retention / migrations                             │
;;   └──────────────────────────────────────────────────────────────┘
;;
;; ══════════════════════════════════════════════════════

(intent memory
  (version "draft-v0.4.4")
  (parent "v2/intent.lisp :: pillar memory")
  (created "2026-04-19")
  (history
    (v0.1   "4 分类扁平")
    (v0.2   "仿 event-bus 三段式 (ingress/core/egress)")
    (v0.3   "3 成熟模块 + 平铺 + 横切 (embedding/gen-crud 迁出)")
    (v0.3.1 "搜索引擎迁出到 pillar 二 2.6 (搜索是计算不是数据)")
    (v0.4   "4 成熟模块各管自己的表: kb-manager + conversation-logs 两个新模块; skill/specs 归 project-management; event-bus 4 表归 pillar 四")
    (v0.4.1 "SSOT 合并: system_timeline 移除, pillar 四 event_log 成 timeline SSOT (v1.3.0); 总表 61→60")
    (v0.4.2 "4 模块 in/core/out harmonization: board/kb/conv core 统一用 (path/plumbing/helper) 三分法语义")
    (v0.4.3 "CLAUDE.md 分层: 全局 ~/.claude/CLAUDE.md + manager → pillar 五 intent-layer; 项目级 <project>/CLAUDE.md + manager → memory :: project-management")
    (v0.4.4 "action/instruction specs 迁 pillar 五: intent/plan/workflow/user_intents DB 表 + system-level intent*.lisp + workflows*.lisp + flows/*.yaml; memory 只留'项目代码真实状态'的 per-project intent.lisp"))
  (status "草稿 — 大多数 module 已稳定, 可演进")

  (purpose "系统长期记忆 — 4 个业务模块自治 + 底层系统支持层 + 横切")
  (storage "PostgreSQL via sqlx::PgPool")
  (gateway "crates/missiond-core/src/db/ — 唯一 DB 入口")

  (migrated-out
    "embedding-provider → pillar 二 2.2 sonnet-gateway (qwen3 双角色)"
    "gen-crud (Forge 冲压) → pillar 二 2.5 code-generation"
    "search-engines → pillar 二 2.6 search-engines (搜索是计算不是数据)"
    "event-bus 4 表 (event_log / event_subscriptions / blob_storage / dead_letter_queue) → pillar 四 §4.6 persistence-layer")

  ;; ═════════════════════════════════════════════════════════════
  ;;  Scoping Index — 按 project_id 列实际存在归类
  ;; ═════════════════════════════════════════════════════════════
  (scoping-index
    (desc "按 project_id 列实况归类 — 源: migrations/20260410100000_project_id_columns.sql")
    (authoritative-source "crates/missiond-core/migrations/20260410100000_project_id_columns.sql")

    (group scoped-primary
      (rationale "核心业务, 项目隔离是本质需求")
      (tables
        (projects  :role "registry — project_id 是主键"        :owned-by "module project-management")
        (knowledge :role "goal: KB 省 token"                   :owned-by "module kb-manager" :has-column true)))

    (group scoped-secondary
      (rationale "有 project_id 列, 顺带覆盖")
      (tables
        (conversations :has-column true :owned-by "module conversation-logs")
        (board_tasks   :has-column true :owned-by "module board")))

    (group inherited-scope
      (rationale "自身无 project_id, 通过 parent FK 推断")
      (from-conversations-session-id
        :count 11
        :tables "conversation_messages / turns / events / tool_calls / topic_vectors / labels; message_narrations / narration_cursors / embeddings / translations / labels"
        :owned-by "module conversation-logs")
      (from-knowledge-id
        :count 3
        :tables "knowledge_edges / kb_ast_links / kb_operation_queue"
        :owned-by "module kb-manager")
      (from-board-tasks-id
        :count 1
        :tables "board_task_notes"
        :owned-by "module board"))

    (group global-infrastructure
      (rationale "系统级, 本就不应按项目分")
      :owned-by "category system-support"
      :examples "daemon_state / credentials / inbox / events / tasks / slot_sessions / watcher_cursors / consumer_watermarks / reconcile_watermarks / backfill_*"
      :note "v0.4.1: system_timeline 已移除 — 合并进 pillar 四 event_log (见 migration-log)")

    (group candidates-for-promotion
      (rationale "⚠ 目前全局无 project_id, 按项目分可能有价值")
      (candidates
        (retrospective_results :benefit "项目级复盘归档"     :owner-note "module conversation-logs; 可经 session_id → conversations.project_id 推断")
        (token_usage_ledger    :benefit "项目级成本追踪 — 最想看的指标" :owner-note "category system-support")
        (prompt_snapshots      :benefit "项目级 prompt 调优"  :owner-note "module board; PK=task_id 可经 board_tasks.project_id 推断")
        (incidents             :benefit "项目级告警分级"     :owner-note "category system-support")
        (gemini_requests       :benefit "项目级 LLM 使用模式" :owner-note "category system-support")
        (agent_questions       :benefit "项目级 agent 问题集" :owner-note "module board; 可经 task_id → board_tasks.project_id 推断")
        (slot_tasks            :benefit "项目级 slot 任务"   :owner-note "category system-support compute-runtime; 可能应归 pillar 二")
        (user_intents          :benefit "项目级意图识别"     :owner-note "pillar 五 intent-layer (v0.4.4 迁出); 缺 project_id 列")
        (intent plan workflow  :benefit "specs 三表应加 project_id" :owner-note "pillar 五 intent-layer (v0.4.4 迁出); dead schema, 需实现")
        (ast_nodes ast_file_meta beacons beacon_nodes :benefit "项目级代码索引" :owner-note "module kb-manager; 缺列")
        (skill_topics skill_blocks skill_versions skill_executions :benefit "项目私有技能库" :owner-note "module project-management; 缺列")
        (image_descriptions    :benefit "项目级图片注释"    :owner-note "category system-support; 独立按 hash 去重")
        (router_chat_archive   :benefit "项目级 router 归档" :owner-note "category system-support")
        (flows-yaml-files      :benefit "项目私有 flow 模板" :kind "文件, 非 DB"))))


  ;; ═════════════════════════════════════════════════════════════
  ;;  Table Catalog — 61 张 PG 表按模块 ownership 归类
  ;; ═════════════════════════════════════════════════════════════
  (table-catalog
    (desc "migrations/*.sql 的 CREATE TABLE 实际统计 — 按 module / category ownership 分组")
    (total 60)
    (total-note "61 migrations 定义, -1 = system_timeline 合并进 event_log (v0.4.1 SSOT 整合)")
    (source "crates/missiond-core/migrations/")

    (by-owner module-project-management (count 5)
      (projects         :purpose "项目注册 — id/path/intent_path/active/slots/github_url/vault")
      (skill_topics     :purpose "技能主题 (顶层分类)")
      (skill_blocks     :purpose "技能内容块")
      (skill_versions   :purpose "技能版本")
      (skill_executions :purpose "技能执行记录")
      (moved-out-v0.4.4 "intent / plan / workflow / user_intents 4 张迁到 pillar 五 intent-layer"))

    (by-owner pillar-five-intent-layer (count 4)
      :owned-section ".missiond/v2/intent.lisp :: pillar intent-layer :: section action-instruction-specs"
      :note "v0.4.4 从 memory project-management 迁入 — action/instruction 层, 非项目数据"
      (intent       :purpose "项目 intent 规约 DB 表 (20260420, schema-only)")
      (plan         :purpose "执行计划 DB 表 (20260420, schema-only)")
      (workflow     :purpose "工作流模板 DB 表 (20260420, schema-only)")
      (user_intents :purpose "用户意图识别记录 DB 表"))

    (by-owner module-board (count 4)
      (board_tasks      :purpose "任务队列 — 27 列, 7 态 FSM"                    :scoping secondary)
      (board_task_notes :purpose "任务附注"                                      :scoping inherited)
      (agent_questions  :purpose "Agent 卡住时提的问题 (FK→board_tasks nullable)")
      (prompt_snapshots :purpose "task 执行 prompt 快照 + KB citation 审计"      :note "PK=task_id"))

    (by-owner module-kb-manager (count 9)
      (knowledge           :purpose "语义级记忆 40+ category"    :scoping primary)
      (knowledge_edges     :purpose "KB 条目间关系图"            :scoping inherited)
      (kb_access_log       :purpose "KB 共访问记录 (prefetch 触发)" :scoping inherited)
      (kb_operation_queue  :purpose "KB 变更异步队列"            :scoping inherited)
      (kb_ast_links        :purpose "KB ↔ AST 节点关联")
      (ast_nodes           :purpose "AST 节点 — 文件/函数/类结构化")
      (ast_file_meta       :purpose "文件级元数据 (hash/size/modified)")
      (beacons             :purpose "代码 beacon — 关注点标记")
      (beacon_nodes        :purpose "beacon 关联的 AST 节点"))

    (by-owner module-conversation-logs (count 14)
      (conversations              :purpose "会话主表 — session_id/summary/project_id/engine_type" :scoping secondary)
      (conversation_messages      :purpose "消息原始记录 (PTY JSONL 来源)")
      (conversation_turns         :purpose "turn 级切分 (user ↔ assistant)")
      (conversation_events        :purpose "会话事件流 (tool use / status change)")
      (conversation_tool_calls    :purpose "工具调用详情")
      (conversation_topic_vectors :purpose "话题向量 — 语义聚类")
      (conversation_labels        :purpose "会话级打标")
      (message_embeddings         :purpose "消息级向量 (halfvec 512)")
      (message_embedding_skips    :purpose "跳过 embedding 的记录 (独立写入路径)")
      (message_narrations         :purpose "消息 LLM 摘要 (briefing 产)")
      (narration_cursors          :purpose "narration 游标 (防重)")
      (message_translations       :purpose "消息多语种翻译")
      (message_labels             :purpose "消息级打标")
      (retrospective_results      :purpose "会话复盘 JSON (PK=session_id)"))

    (by-owner pillar-four-event-bus (count 4)
      :owned-section "pillar 四 §4.6 persistence-layer"
      :note "这 4 表不在 memory pillar 管辖, 只在此列名方便索引")

    (by-owner category-system-support (count 20)
      (incidents             :purpose "告警/异常聚合"         :scoping candidate)
      (token_usage_ledger    :purpose "LLM 调用成本追踪"      :scoping candidate)
      (gemini_requests       :purpose "Gemini API 调用日志"   :scoping candidate)
      (gemini_file_uploads   :purpose "Gemini 文件上传缓存")
      (router_chat_archive   :purpose "Router 聊天归档"       :scoping candidate)
      (image_descriptions    :purpose "图片描述缓存 (vision_worker 写, 按 hash 去重, 无 FK)")
      (daemon_state          :purpose "daemon 级全局状态")
      (credentials           :purpose "凭据存储 (加密)")
      (inbox                 :purpose "收件箱 (老表, 疑似 deprecated)")
      (events                :purpose "事件表 (老版, 疑似 deprecated)")
      (tasks                 :purpose "任务表 (老版, 疑似 deprecated)")
      (slot_sessions         :purpose "槽位会话生命周期"     :owner-alt "可归 pillar 二")
      (slot_tasks            :purpose "槽位任务队列"         :owner-alt "可归 pillar 二")
      (dynamic_slots         :purpose "按需创建的动态槽位"   :owner-alt "可归 pillar 二")
      (reconcile_watermarks  :purpose "对账游标")
      (gemini_cli_watermarks :purpose "Gemini CLI 水位游标")
      (watcher_cursors       :purpose "观察者游标")
      (consumer_watermarks   :purpose "消费者水位")
      (backfill_progress     :purpose "回填进度追踪")
      (backfill_failures     :purpose "回填失败记录"))

    (legacy-note "⚠ tasks / inbox / events 疑似老版 schema 不再活跃; slot_* 可能属 pillar 二 worker"))


  ;; ═════════════════════════════════════════════════════════════
  ;;  Non-DB Forms — 数据库之外的记忆载体
  ;; ═════════════════════════════════════════════════════════════
  (non-db-forms
    (desc "Lisp / YAML / Markdown / 外部流 / 向量 / 大对象 6 种载体")

    (form lisp-spec-files
      (desc "Lisp 自我描述文件 — memory 层只剩 per-project 代码快照 (v0.4.4)")
      :moved-out "system-main / system-detail / workflow-templates → pillar 五 intent-layer :: section action-instruction-specs (那些是 action/instruction, 本层只留 factual code state)"
      (variant project-intent
        :path "<project>/.missiond/intent.lisp"
        :scope per-project
        :writer "pillar 二 2.3 lisp-survey-worker"
        :reader "mission_intent tool (from memory :: project-management :: mcp-intent-view)"
        :nature "factual code state snapshot — 描述项目代码真实状态"
        :purpose "每项目的架构画像, goal-1 直接服务对象"
        :managed-by "module project-management :: path project-code-snapshot"))

    ;; yaml-flow-templates 迁出到 pillar 五 intent-layer :: section action-instruction-specs

    (form markdown-handwritten-memories
      (desc "Markdown + YAML frontmatter 人工手写 — 项目级 + auto-memory vault")
      :note "全局 ~/.claude/CLAUDE.md 已迁到 pillar 五 intent-layer (元层, 不属业务记忆)"
      (variant project-claudemd
        :path "<project>/CLAUDE.md"
        :scope per-project
        :purpose "项目级 Claude 指令 (随项目 git 版本化, 每会话加载)"
        :managed-by "module project-management :: helper project-claudemd-manager")
      (variant auto-memory-vault
        :path "~/.claude/projects/{encoded}/memory/*.md"
        :index "~/.claude/projects/{encoded}/memory/MEMORY.md"
        :types "user / feedback / project / reference"
        :purpose "Agent 跨会话持久记忆"
        :managed-by "module project-management :: helper project-memories-vault"))

    (form external-source-streams
      (desc "外部原始流, worker 消费入 DB")
      (source pty-session-jsonl
        :path "~/.claude/projects/{encoded}/*.jsonl"
        :producer "Claude Code CLI (外部)"
        :consumer "pillar 二 2.3 :: conversation-logger worker"
        :writes-to "module conversation-logs :: 4 张 conversation_* 表"
        :idempotency "(session_id, turn_id) 去重"
        :claimed-by "module conversation-logs :: non-db-source")
      (source git-commit-history
        :path "<project>/.git/"
        :producer "git"
        :consumer "pillar 二 2.3 :: lisp-survey-worker (via ContextualCommitDetected)"
        :triggers "更新项目 intent.lisp"
        :debounce "60s per project_id"))

    (form embedding-vectors
      (desc "pgvector 512-dim 二进制 — 存 DB 列")
      :columns "knowledge.embedding_vec / conversations.summary_embedding / conversation_topic_vectors.vec / message_embeddings.vec"
      :dim 512
      :provider "pillar 二 2.2 sonnet-gateway (qwen3)"
      :generator "pillar 二 2.3 embedding-worker"
      :consumer "pillar 二 2.6 search-engines :: vector-hnsw"
      :invariant "禁止降级兜底")

    (form side-channel-blobs
      (desc "claim-check 大对象")
      (location blob-storage-pg
        :table "blob_storage"
        :owned-by "pillar 四 §4.6"
        :purpose "event-bus >8KB payload side-channel")
      (location gemini-file-remote
        :table "gemini_file_uploads (引用)"
        :actual-storage "Gemini 服务端"
        :owned-by "category system-support")))


  ;; ═════════════════════════════════════════════════════════════
  ;;  模块 1: Project Management
  ;;  项目作为记忆的作用域单位; 管 projects + specs + skills
  ;; ═════════════════════════════════════════════════════════════
  (module project-management
    (desc "项目管理: 注册 + 作用域 + 规约(intent/plan/workflow) + 技能(skills)")
    (maturity "成熟")
    :migrated-in-v0.4 "skills (4 表) + specs (4 表) 从 system-support/project-specs 并入本模块"

    (primary-goals
      (goal-1 per-project-code-snapshot-access
        :problem  "agent 调 MCP 不知当前项目代码状态, 需多步 grep/tree 扫"
        :solution "ProjectRegistry::resolve(cwd) → project_id → projects.intent_path → 读 <project>/.missiond/intent.lisp (代码真实状态快照)"
        :benefit  "每次 MCP 调用省 3-5 步代码导航"
        :scope    "per-project intent.lisp 文件 (factual code state snapshot) — 非 instruction/action"
        :moved-out "intent/plan/workflow DB 表 + system-level lisp + workflow 方法论 → pillar 五 intent-layer (v0.4.4)")
      (goal-2 kb-token-economy
        :problem  "KB 全局查召回无关项目条目, 浪费 token"
        :solution "WHERE knowledge.project_id = $X OR IS NULL (委托 module kb-manager)"
        :benefit  "LLM context 更聚焦, token 使用下降"
        :delegates-to "module kb-manager")
      (goal-3 project-skill-scope
        :problem  "全局 skill 对具体项目可能不适用"
        :solution "skill_* 表加 project_id (待实现, 见 scoping-index :: candidates)"
        :benefit  "项目私有 skill, 避免跨项目污染"))

    (module-ingress
      (desc "项目注册 + spec 变更 + skill 变更 + 作用域传播")

      (writer mcp-project-mutation
        :tools  "mission_project init / sync / set_active / vault_sync / import_universe / survey"
        :writes "projects"
        :code   "daemon/src/handlers/knowledge/project.rs")

      (writer lisp-survey-worker-snapshot
        :kind   "sonnet"
        :code   "crates/missiond-daemon/src/workers/sonnet/lisp_survey_worker.rs"
        :writes "<project>/.missiond/intent.lisp (per-project 代码状态快照 FILE, 非 DB)"
        :trigger "ContextualCommitDetected → slot dispatch"
        :debounce "60s per project_id"
        :note   "DB specs (intent/plan/workflow/user_intents) 归 pillar 五, 不在本模块")

      (writer mcp-skill-mutation
        :tools  "mission_skill_mutate"
        :writes "skill_topics / skill_blocks / skill_versions / skill_executions"
        :code   "daemon/src/handlers/knowledge/skill.rs")

      (writer project-memory-vault-edit
        :source "用户手动 + Claude Code 文件操作"
        :writes "~/.claude/projects/{encoded}/memory/*.md"
        :kind   "文件系统, 不走 DB")

      (writer project-scope-propagation
        :desc   "其他模块 writer 插入时自动打 project_id"
        :writers-list
          ("conversation-logger → conversations.project_id"
           "mcp-kb-mutation → knowledge.project_id"
           "mcp-board-lifecycle → board_tasks.project_id"
           "retro-worker → retrospective_results (inherited 继承)")
        :resolve-via "ProjectRegistry::resolve(cwd)"))

    (module-core
      (desc "主路径 (path) + 共用基础 (plumbing) + 辅助 (helper)")

      ;; ── 主路径 ──
      (path project-code-snapshot
        :serves goal-1
        :flow  "cwd → ProjectRegistry::resolve → project_id → projects.intent_path → 读 <project>/.missiond/intent.lisp 文件"
        :entry "mission_intent read / section / summary / list"
        :nature "factual code state snapshot — 描述项目代码真实状态 (由 lisp-survey-worker 基于 commit 差量更新)"
        :file-form "<project>/.missiond/intent.lisp"
        :out-of-scope "DB specs (intent/plan/workflow/user_intents) 与 system-level lisp 在 pillar 五")

      (path project-kb-access
        :serves goal-2
        :delegates-to "module kb-manager :: mcp-kb-query (带 project_scoped=true)"
        :rule  "WHERE knowledge.project_id = $X OR IS NULL"
        :note  "本 path 不自己实现 KB, 通过 scope-mechanism 让 kb-manager 过滤")

      (path project-skill-access
        :serves goal-3
        :entry "mission_skill_query / mission_skill_context (按项目过滤)"
        :tables "skill_topics / skill_blocks / skill_versions / skill_executions"
        :todo  "skill_* 加 project_id 列 + 扩 scope-mechanism applies-to")

      ;; ── plumbing ──
      (plumbing project-registry
        (desc "核心基础 — projects 表 + 内存索引 + cwd 解析")
        :tables "projects"
        :schema-cols "id(PK) / path(unique) / intent_path / active / slots[] / github_url / timestamps"
        :in-memory "启动从 PG 加载; longest-prefix 路径匹配; 运行期只读"
        :migration "20260410000000_projects.sql + 20260410200000 seed"
        :resolver "ProjectRegistry::resolve(cwd) / exclusive_slots / list_active"
        :code "crates/missiond-core/src/{types,db/pg}/project.rs + daemon/src/state.rs")

      (plumbing scope-mechanism
        (desc "project_id 列 + 查询规则")
        :rule "WHERE project_id = $X OR IS NULL"
        :semantics "NULL = 全局共享; 非 NULL = 项目私有"
        :applies-to "4 张实际有列: projects / knowledge / conversations / board_tasks"
        :candidates "~15 张可升级 (见 scoping-index :: candidates-for-promotion)"
        :see-also "顶层 scoping-index / table-catalog")

      ;; ── helper ──
      (helper project-context-aggregator
        (desc "mission_project context 视图 — 多源聚合观察项目当前状态")
        :code "daemon/src/handlers/knowledge/project.rs :: build_*"
        :aggregates
          ("build_intent_summary: 读 intent.lisp → survey-date / pillar 列表"
           "conversation_stats_by_project: COUNT / status (借 module conversation-logs)"
           "kb_stats_by_project: BY category (借 module kb-manager)"
           "skill-status: 项目 skill 执行情况"
           "slot-status: 项目关联的 compute_slots"))

      (helper project-memories-vault
        (desc "项目专属的人工手写记忆库")
        :storage "~/.claude/projects/{encoded}/memory/*.md"
        :format "Markdown + YAML frontmatter(name/type/description)"
        :code "daemon/src/handlers/knowledge/project_memory.rs"
        :invariant "path-traversal 防护; 独立于 DB")

      (helper project-claudemd-manager
        (desc "项目级 <project>/CLAUDE.md 的读/写/reload 管理 — Claude 每会话加载")
        :path "<project>/CLAUDE.md"
        :scope per-project
        :nature "Markdown 明文, 随项目 git 版本化"
        :readers "Claude Code 系统启动时读, 拼 system prompt"
        :writers "用户手动 / Claude Code 文件编辑 (Edit tool)"
        :code "TBD — 目前 Claude Code 直接读文件, 无 daemon 侧 manager; 未来可补 MCP tool (mission_project_claudemd read/edit/reload)"
        :status "文件层存在, DB 层无 manager"
        :cross-ref "全局 ~/.claude/CLAUDE.md 在 pillar 五 intent-layer :: global-claudemd (非本模块)"))

    (module-egress
      (desc "项目级别的读取")

      (reader mcp-project-views
        :tools "mission_project get / list / context / memories / sync / vault_sync"
        :reads "projects + 多表聚合 + vault")

      (reader mcp-intent-view
        :tool "mission_intent read/section/summary/list"
        :reads "<project>/.missiond/intent.lisp 文件 (only — 描述项目代码真实状态)"
        :not-reads "intent DB 表 (那是 action/instruction specs, 归 pillar 五)")

      (reader mcp-skill-view
        :tools "mission_skill_query / mission_skill_context"
        :reads "skill_topics / blocks / versions / executions")

      (reader frontend-project-stream
        :emits "项目列表 / 活跃切换 / 项目元数据变更"
        :via "pillar 六 6.2 ws-server"))

    (module-tables-owned
      (desc "本模块独占 5 张表 (v0.4.4: specs 4 张已移到 pillar 五)")
      (tables
        (projects         :status "✓ active")
        (skill_topics     :status "✓ active — mission_skill_* MCP")
        (skill_blocks     :status "✓ active")
        (skill_versions   :status "✓ active")
        (skill_executions :status "✓ active"))
      (count 5)
      (removed-in-v0.4.4 "intent / plan / workflow / user_intents 迁到 pillar 五 intent-layer (都是 action/instruction 层, 非项目数据)")
      (non-db-forms-owned
        (lisp-file "<project>/.missiond/intent.lisp (per-project 代码快照, see non-db-forms :: lisp-spec-files)")
        (md-vault "~/.claude/projects/{encoded}/memory/*.md (see non-db-forms :: markdown-handwritten-memories)")
        (project-claudemd "<project>/CLAUDE.md (项目指令, via helper project-claudemd-manager)"))))


  ;; ═════════════════════════════════════════════════════════════
  ;;  模块 2: Board (看板)
  ;;  任务队列 — worker 的唯一输入面
  ;; ═════════════════════════════════════════════════════════════
  (module board
    (desc "任务队列 — 所有可分发工作的权威存储, worker 的唯一输入面")
    (maturity "成熟")
    :migrated-from "v0.3.1 module board (内容不变, v0.4 仅补 owned-tables)"

    (primary-goals
      (goal-1 task-queue-authority
        :problem  "分布式 worker 要原子地领取/推进任务, 避免重复执行"
        :solution "CAS 原子 claim + lease_expires 回收 + 7 态 FSM"
        :benefit  "唯一事实来源, 并发安全")
      (goal-2 dag-orchestration
        :problem  "复杂流程需要 DAG 依赖 + 级联重试"
        :solution "depends_on(JSONB) + retry_cascade BFS"
        :benefit  "支撑 autopilot 自主编排"))

    (module-ingress
      (desc "任务创建 + 状态推进 + 级联 + 问题 + 执行存档的写入路径")

      (writer mcp-board-lifecycle
        :tools "mission_board_create / update / claim / decompose / retry / note_add / delete"
        :count 7
        :writes "board_tasks / board_task_notes"
        :code "daemon/src/handlers/knowledge/board.rs")

      (writer autopilot-engine
        :code "daemon/src/engine/intent_engine/autopilot.rs"
        :tick "5-10s"
        :writes "board_tasks (CAS claim / status 推进 / lease 回收)")

      (writer flow-engine-v2
        :code "daemon/src/engine/flow/*.rs"
        :writes "board_tasks.flow_context (每节点 persist)")

      (writer mcp-question-lifecycle
        :tool "mission_question (独立 MCP 工具, 非 board 工具面)"
        :writes "agent_questions (+ 自动 UPDATE 关联 board_tasks.status='blocked')"
        :code "daemon/src/handlers/.../question.rs (创建问题时 CAS 关联 task)")

      (writer autopilot-prompt-snapshot
        :code "crates/missiond-daemon/src/engine/intent_engine/autopilot.rs :: save_prompt_snapshot"
        :writes "prompt_snapshots"
        :purpose "task 执行时的 prompt + KB 引用存档 — KB citation 审计 + prompt 调优"
        :note "PK 是 task_id, 故归本模块"))

    (module-core
      (desc "主路径 (path) + 数据/FSM/操作/事件 (plumbing) + 附属 (helper)")

      ;; ── 主路径: 任务生命周期 (serves goal-1 + goal-2) ──
      (path task-queue-lifecycle
        :serves (goal-1 goal-2)
        :flow  "create → claim (CAS) → running → verifying / done / failed → optional retry (DAG cascade)"
        :entry "mission_board_* (7 个) + autopilot tick + flow-engine 节点推进"
        :tables "board_tasks + board_task_notes"
        :see-plumbing "data-model / state-machine / core-operations / events-emitted")

      ;; ── plumbing: 支撑主路径的数据 + 逻辑 ──
      (plumbing data-model
        :table "board_tasks"
        :columns 27
        :grouping "身份 / 内容 / 生命周期 / 占用 / 层级 / 执行 / 流程 / 作用域 / 去重 / UI"
        :indexes 5
        :code "crates/missiond-core/src/db/board.rs")

      (plumbing state-machine
        :states (open running verifying done failed blocked skipped)
        :transitions "open→running(CAS) / open→blocked / running→verifying/done/failed / blocked→open / terminal→open(retry)"
        :atomicity "open→running 是 SQL CAS"
        :recovery "lease_expires_at 超期 → recover_stale_running_tasks")

      (plumbing core-operations
        :operations 8 "create / claim / update / decompose / retry / note-add / delete / query"
        :code "crates/missiond-core/src/db/board.rs + handlers/knowledge/board.rs")

      (plumbing events-emitted
        :variants "BoardTaskCreated / StatusChanged / NoteAdded / Claimed / Deleted / Updated"
        :persisted-via "pillar 四 event_log (SSOT; v0.4.1 合并后不再走 system_timeline)")

      ;; ── helper: 附属表 + 独立生命周期 bridge ──
      (helper task-notes
        :table "board_task_notes"
        :scoping "inherited via board_tasks.id"
        :purpose "附注不改 status")

      (helper agent-questions
        :table "agent_questions"
        :schema "task_id TEXT REFERENCES board_tasks(id) NULLABLE + status (pending/answered/dismissed) + target (user/master)"
        :purpose "Agent 卡住时提问, 等待用户或其他 agent 回答"
        :relationship "物理上有 FK 到 board_tasks, 逻辑上独立生命周期 (status 独立)"
        :creation-side-effect "创建问题时 CAS UPDATE board_tasks SET status='blocked' WHERE id=task_id"
        :mcp "mission_question (独立 MCP 工具, 非 board 工具面)")

      (helper prompt-snapshot
        :table "prompt_snapshots"
        :schema "task_id PRIMARY KEY / prompt / cited_kb_ids / category / task_outcome / created_at"
        :purpose "task 执行时的 prompt 快照 + KB 引用审计"
        :writer "autopilot.rs :: save_prompt_snapshot"
        :readers "MCP prompt 调优工具 (待实现) / 复盘分析"
        :cross-module-note "cited_kb_ids 关联 kb-manager :: knowledge; 但 PK 是 task_id 故归 board"))

    (module-egress
      (desc "MCP 查询 + 前端 WS 流 + autopilot 内部扫描")

      (reader mcp-board-query
        :tool "mission_board_query"
        :actions "list / get / search / summary / clear_done")

      (reader frontend-board-stream
        :source "pillar 四 event_log subscribe BoardEvent"
        :emits "BoardTask* 事件 → 前端实时更新"
        :via "daemon/src/bus/ws_bridge.rs")

      (reader autopilot-tick-scan
        :query "WHERE auto_execute=1 AND status='open'"
        :note "autopilot 自读自写"))

    (module-tables-owned
      (desc "本模块独占 4 张表")
      (tables board_tasks board_task_notes agent_questions prompt_snapshots)
      (count 4)
      (added-in-v0.4-revision "prompt_snapshots 从 kb-manager 挪入 (PK 是 task_id, 归属任务执行)")))


  ;; ═════════════════════════════════════════════════════════════
  ;;  模块 3: KB Manager (知识库管理)  NEW
  ;;  语义级记忆 + 代码索引 + 访问审计
  ;; ═════════════════════════════════════════════════════════════
  (module kb-manager
    (desc "知识库管理 — 语义记忆 + 代码索引 + 变更审计 + prompt 存档")
    (maturity "成熟")
    :migrated-from "v0.3.1 category system-support :: knowledge-db-layer + table-catalog :: domain knowledge-kb"

    (primary-goals
      (goal-1 semantic-recall
        :problem  "跨会话记忆散落在 jsonl 流里, 无法语义检索"
        :solution "40+ category 结构化存储 + 向量索引(delegated to pillar 二 2.6)"
        :benefit  "Agent / 用户可按语义 / 关键词 / 标签三路找回记忆"
        :serves-also "project-management goal-2 (project-scoped KB)")
      (goal-2 code-awareness
        :problem  "LLM 对代码库的理解每次都要从头扫"
        :solution "AST 结构化存储 + KB ↔ AST 链接"
        :benefit  "mission_code_search 可语义 + 结构并查")
      (goal-3 audit-mutation-trail
        :problem  "KB 条目 append-only 不够, 还需知道谁改了谁读了"
        :solution "kb_access_log (读审计) + kb_operation_queue (写队列)"
        :benefit  "KB 演化可追溯"))

    (module-ingress
      (desc "KB 变更写入 + 代码索引维护 + 访问审计")

      (writer mcp-kb-mutation
        :tools  "mission_kb_mutate / mission_kb_remember / mission_kb_batch_set_project"
        :writes "knowledge (+ kb_operation_queue async)"
        :code   "daemon/src/handlers/knowledge/kb.rs")

      (writer worker-arch-maintenance
        :kind   "sonnet"
        :code   "crates/missiond-daemon/src/workers/sonnet/arch_maintenance_worker.rs"
        :writes "knowledge (category=architecture)"
        :trigger "定期扫描项目 intent.lisp + 代码变更")

      (writer worker-experience-harvester
        :kind   "local"
        :code   "crates/missiond-daemon/src/workers/local/experience_harvester.rs"
        :source "module conversation-logs 的 conversations"
        :writes "knowledge (category=bugfix/policy/memory)"
        :purpose "从会话挖掘经验 → KB")

      (writer worker-tagger-chunker
        :kind   "local"
        :code   "crates/missiond-daemon/src/workers/local/tagger_chunker.rs"
        :writes "knowledge (chunks + tags)"
        :purpose "分块 + 打标签 + 生成 kb_ast_links")

      (writer worker-ast-sync
        :kind   "local"
        :code   "crates/missiond-daemon/src/workers/local/ast_sync_worker.rs"
        :writes "ast_nodes / ast_file_meta / beacons / beacon_nodes"
        :purpose "代码结构索引维护 — 文件变更时增量更新")

      (writer worker-embedding-cross
        :kind   "sonnet (cross-module writer)"
        :code   "crates/missiond-daemon/src/workers/sonnet/embedding_worker.rs"
        :writes "knowledge.embedding_vec"
        :also-writes-in-other-module "conversation-logs: message_embeddings + conversation_topic_vectors"
        :purpose "生成 KB 条目向量供向量检索"
        :provider "qwen3 via sonnet-gateway (pillar 二 2.2)")

      (writer context-pipeline-kb-audit
        :code   "crates/missiond-daemon/src/context/context_pipeline.rs :: kb_log_co_access"
        :writes "kb_access_log"
        :purpose "KB prefetch 时记录共访问关系 — 用于 KB 相关性挖掘"
        :note "不是通用 middleware, 只在 context-pipeline prefetch 路径触发"))

    (module-core
      (desc "主路径 (path) + KB 表族 (plumbing) + 代码索引 (plumbing) + helper (audit/queue)")

      ;; ── 主路径: 2 条, 各对应 goal ──
      (path kb-semantic-recall
        :serves goal-1
        :flow  "write → knowledge (+ embedding via worker) → search (三路融合) → read"
        :entry "mission_kb_mutate/remember (write) + mission_kb_query/search (read)"
        :tables "knowledge + knowledge_edges"
        :delegates-search-to "pillar 二 2.6 search-engines")

      (path kb-code-awareness
        :serves goal-2
        :flow  "代码变更 → ast-sync-worker 更新索引 → kb_ast_links 关联 → mission_code_search"
        :entry "mission_code_search / mission_universe_graph"
        :tables "ast_nodes + ast_file_meta + beacons + beacon_nodes + kb_ast_links"
        :maintained-by "worker-ast-sync (local)")

      ;; ── plumbing: KB 表族 ──
      (plumbing knowledge-store
        (desc "语义级记忆主表 — 40+ category, 每条独立知识片段")
        :table "knowledge"
        :schema-cols "id / key / category / content / embedding_vec(512) / project_id / tags / access_count / created_at / updated_at"
        :code "crates/missiond-core/src/db/knowledge.rs"
        :scoping "has project_id column (primary scoped)")

      (plumbing knowledge-graph
        (desc "KB 条目之间的关系图 — 支撑 universe_graph")
        :table "knowledge_edges"
        :schema-cols "id / from_id / to_id / relation_type / weight / created_at"
        :scoping "inherited via knowledge.id")

      (plumbing kb-ast-linkage
        (desc "KB 条目 ↔ AST 节点的双向关联")
        :table "kb_ast_links"
        :purpose "支撑 mission_code_search 从 KB 跳到代码, 或从代码跳到相关 KB")

      (plumbing code-indexing
        (desc "代码结构化索引 — ast_nodes / ast_file_meta / beacons / beacon_nodes")
        :tables (ast_nodes ast_file_meta beacons beacon_nodes)
        :code "crates/missiond-core/src/db/ast.rs"
        :maintained-by "worker-ast-sync"
        :consumed-by "mission_code_search / universe_graph / kb-ast-linkage"
        :scoping "global (候选 per-project)")

      (plumbing search-delegation
        (desc "搜索走 pillar 二 2.6 search-engines 四路融合")
        :engines "HNSW vector + GIN FTS + trigram + tag"
        :delegates-to "pillar 二 2.6 search-engines :: fusion-ranker"
        :see-also "pillar 二 2.6")

      (plumbing project-scope-consumption
        (desc "使用 module project-management :: scope-mechanism 过滤 KB 查询")
        :rule "WHERE knowledge.project_id = $X OR IS NULL"
        :owner "scope-mechanism 所有权在 project-management, 本模块只消费")

      ;; ── helper: 审计 + 异步队列 (serves goal-3) ──
      (helper access-audit
        :serves goal-3
        (desc "KB 访问审计 — 记录共访问关系, 用于相关性挖掘")
        :table "kb_access_log"
        :writer "context_pipeline.rs :: kb_log_co_access (非通用 middleware)"
        :scoping "inherited"
        :retention "append-only; 按时间 DELETE (待策略)")

      (helper operation-queue
        :serves goal-3
        (desc "KB 变更异步队列 — 大规模 mutate 走队列避开阻塞")
        :table "kb_operation_queue"
        :scoping "inherited"
        :consumer "后台批处理 worker (待确认)"))

    (module-egress
      (desc "KB 查询 + 搜索 + 代码搜索 + Context 拼接")

      (reader mcp-kb-query
        :tools "mission_kb_query / mission_kb_search / mission_kb_ops"
        :reads "knowledge + knowledge_edges"
        :invokes "pillar 二 2.6 search-engines")

      (reader mcp-insight-recall
        :tools "mission_insight / mission_memory"
        :focus "综合洞察 / 记忆召回"
        :reads "knowledge")

      (reader mcp-code-search
        :tool "mission_code_search"
        :reads "knowledge + ast_nodes + beacons + kb_ast_links"
        :focus "代码语义 + 结构并查")

      (reader mcp-universe-graph
        :tool "mission_universe_graph"
        :reads "knowledge + knowledge_edges (跨项目)"
        :focus "实体 / 关系图生成")

      (reader context-pipeline-retrieval
        :code "crates/missiond-daemon/src/context/{pipeline,retrieval}.rs"
        :purpose "为 LLM 调用拼 prompt"
        :reads "knowledge (向量 + 最近)"
        :note "记忆最密集的消费者")

      (reader worker-code-prefetch
        :kind "local"
        :code "crates/missiond-daemon/src/workers/local/code_prefetch.rs"
        :reads "ast_nodes / beacons"
        :purpose "AST 混合搜索引擎 (FTS5 + embedding RRF) — 只读, 不写"))

    (module-tables-owned
      (desc "本模块独占 9 张表")
      (tables knowledge knowledge_edges kb_access_log kb_operation_queue kb_ast_links
              ast_nodes ast_file_meta beacons beacon_nodes)
      (count 9)
      (removed-in-v0.4-revision "prompt_snapshots 挪到 module board (主键 task_id 属任务执行归属)")))


  ;; ═════════════════════════════════════════════════════════════
  ;;  模块 4: Conversation Logs (会话日志)  NEW
  ;;  Claude Code / Gemini / Codex 三类 CLI 引擎的完整会话
  ;; ═════════════════════════════════════════════════════════════
  (module conversation-logs
    (desc "三类 CLI 引擎(Claude Code / Gemini / Codex)的会话完整记录 + 二次分析")
    (maturity "成熟")
    :migrated-from "v0.3.1 category system-support :: conversation-db-layer + table-catalog :: domain conversations"

    (primary-goals
      (goal-1 complete-session-recording
        :problem  "Claude Code 等 CLI 会话散在 jsonl 流里, 不可检索/回放"
        :solution "PTY JSONL → 结构化 DB (conversations + messages + turns + events)"
        :benefit  "所有会话可查询、按项目隔离、支持二次分析")
      (goal-2 multi-engine-unified
        :problem  "Claude Code / Gemini / Codex 三种 CLI 格式各异"
        :solution "统一的 conversations 表 + engine-specific ingestion worker"
        :benefit  "跨引擎查询一套工具")
      (goal-3 analysis-layer
        :problem  "原始消息 JSONL 难以直接用于 context 拼接 / 复盘"
        :solution "briefing (摘要) + embedding (向量) + translation (多语种) + labels (打标) + retrospective (复盘)"
        :benefit  "多层次派生视图, 服务不同消费场景"))

    (module-ingress
      (desc "3 引擎专项摄入 + 多种二次分析 worker")

      ;; ── 三引擎原始摄入 ──
      (writer worker-conversation-logger
        :kind     "local"
        :code     "crates/missiond-daemon/src/workers/local/conversation_logger.rs"
        :source   "~/.claude/projects/{encoded}/*.jsonl (PTY JSONL, see non-db-forms)"
        :writes   "conversations / conversation_messages / conversation_turns / conversation_events"
        :idempotency "(session_id, turn_id) 去重"
        :engine-primary "Claude Code")

      (writer worker-codex-ingestion
        :kind   "local"
        :code   "crates/missiond-daemon/src/workers/local/codex_ingestion_worker.rs"
        :writes "conversations / conversation_messages (codex engine_type)"
        :engine-primary "Codex")

      (writer worker-gemini-logger
        :kind   "local"
        :code   "crates/missiond-daemon/src/workers/local/gemini_logger.rs"
        :writes "conversations / conversation_messages (gemini engine_type)"
        :engine-primary "Gemini")

      (writer worker-gemini-reconcile
        :kind   "local"
        :code   "crates/missiond-daemon/src/workers/local/gemini_reconcile_worker.rs"
        :writes "conversations (对账校正)"
        :purpose "Gemini 流特殊对账")

      (writer worker-conversation-organizer
        :kind   "local"
        :code   "crates/missiond-daemon/src/workers/local/conversation_organizer.rs"
        :writes "conversation_turns / conversation_tool_calls"
        :purpose "消息流 → turn 结构化 + tool call 抽取")

      ;; ── 二次分析 ──
      (writer worker-briefing
        :kind   "sonnet"
        :code   "crates/missiond-daemon/src/workers/sonnet/briefing_worker.rs"
        :writes "message_narrations + narration_cursors"
        :purpose "LLM 摘要 — 每轮或每会话生成 briefing")

      (writer worker-step-narrator
        :kind   "codex"
        :code   "crates/missiond-daemon/src/workers/codex/step_narrator.rs"
        :writes "message_narrations (codex-specific narrations)")

      (writer worker-embedding
        :kind   "sonnet (cross-module writer)"
        :code   "crates/missiond-daemon/src/workers/sonnet/embedding_worker.rs"
        :writes "message_embeddings / conversation_topic_vectors"
        :also-writes-in-other-module "kb-manager: knowledge.embedding_vec"
        :provider "qwen3 via sonnet-gateway (pillar 二 2.2)"
        :note "不写 conversations.summary_embedding, 也不写 message_embedding_skips (skip 逻辑是 worker 内部控制, 不落此表)")

      (writer worker-translation
        :kind   "sonnet"
        :code   "crates/missiond-daemon/src/workers/sonnet/translation_worker.rs"
        :writes "message_translations"
        :purpose "多语种翻译")

      (writer worker-retro
        :kind   "sonnet"
        :code   "crates/missiond-daemon/src/workers/sonnet/retro_worker.rs"
        :writes "retrospective_results"
        :trigger "会话结束信号 / 手动触发")

      ;; 注: vision_worker 写 image_descriptions 但该表已迁到 category system-support
      ;; (独立无外键的图片缓存, 非 conversation-scoped), 见该 section

      ;; ── MCP 写入 ──
      (writer mcp-conversation-reconcile
        :tools  "mission_conversation_reconcile / mission_conversation_analyze"
        :writes "conversations / conversation_messages"
        :code   "daemon/src/handlers/comm/conversation.rs"))

    (module-core
      (desc "主路径 (path) × 3 + 原始层 plumbing + 派生层 helper + 源与作用域")

      ;; ── 主路径 × 3 (各对应一个 goal) ──
      (path pty-to-structured-db
        :serves goal-1
        :flow  "PTY JSONL → conversation-logger 解包 → conversations/messages/turns/events"
        :entry "worker-conversation-logger (local, tail jsonl)"
        :tables "conversations + conversation_messages + conversation_turns + conversation_events + conversation_tool_calls"
        :idempotency "(session_id, turn_id) 去重")

      (path multi-engine-dispatch
        :serves goal-2
        :flow  "conversations.engine_type 区分 claude-code / gemini / codex"
        :entry "conversation-logger (Claude Code) + codex-ingestion + gemini-logger + gemini-reconcile"
        :invariant "统一 schema, engine-specific ingestion worker")

      (path derived-analysis-layer
        :serves goal-3
        :flow  "原始消息 → 多个派生 worker → 向量 / 摘要 / 翻译 / 标签 / 复盘"
        :entries "worker-embedding / briefing / translation / retro / step-narrator"
        :see-helpers "semantic-vectors / narration-briefing / translation-multilingual / labels-tags / retrospective")

      ;; ── plumbing: 原始层 (path pty-to-structured-db 的底层) ──
      (plumbing session-master
        (desc "会话主表 — session_id / summary / project_id / engine_type / timestamps")
        :table "conversations"
        :scoping "has project_id column (secondary scoped)"
        :engines "claude-code / gemini / codex")

      (plumbing raw-messages
        (desc "消息原始记录 — 从 PTY JSONL 解包")
        :table "conversation_messages"
        :scoping "inherited via conversations.session_id")

      (plumbing turn-structure
        (desc "turn 级切分 — user ↔ assistant 交替")
        :table "conversation_turns"
        :scoping "inherited")

      (plumbing session-events
        (desc "会话内结构化事件 — tool use / status change")
        :table "conversation_events"
        :scoping "inherited")

      (plumbing tool-call-log
        (desc "工具调用详情 — 每次 tool call 的 input/output/status")
        :table "conversation_tool_calls"
        :scoping "inherited")

      ;; ── helper: 派生层 (path derived-analysis-layer 的产物) ──
      (helper semantic-vectors
        (desc "消息 + 话题向量")
        :tables (conversation_topic_vectors message_embeddings message_embedding_skips)
        :generator "worker-embedding (sonnet)"
        :consumer "pillar 二 2.6 search-engines :: vector-hnsw")

      (helper narration-briefing
        (desc "LLM 生成的摘要 + 游标 (防重处理)")
        :tables (message_narrations narration_cursors)
        :generator "worker-briefing (sonnet) + worker-step-narrator (codex)"
        :scoping "inherited")

      (helper translation-multilingual
        (desc "消息翻译 (多语种)")
        :table "message_translations"
        :generator "worker-translation (sonnet)")

      (helper labels-tags
        (desc "会话级 + 消息级打标 (主题 / 质量 / 分类)")
        :tables (conversation_labels message_labels))

      (helper retrospective
        (desc "会话复盘 — JSON 结构化总结")
        :table "retrospective_results"
        :schema "session_id PRIMARY KEY → conversation-scoped (天然关联会话)"
        :generator "worker-retro (sonnet)"
        :consumer "mission_retrospective_manage"
        :scoping-candidate "缺 project_id 列, 可通过 session_id → conversations.project_id 推断")

      ;; ── 源与作用域机制 ──
      (non-db-source pty-jsonl
        (desc "PTY JSONL 是 conversations 的唯一 ingestion 源")
        :path "~/.claude/projects/{encoded-path}/*.jsonl"
        :claimed-from "non-db-forms :: external-source-streams :: pty-session-jsonl")

      (plumbing engine-discrimination
        (desc "conversations.engine_type 区分三类 CLI")
        :values "claude-code / gemini / codex"
        :applied-by "conversation-logger / codex-ingestion / gemini-logger")

      (plumbing project-scope-propagation
        (desc "conversation-logger 摄入时从 PTY path 推断 project_id")
        :resolver "ProjectRegistry::resolve(cwd)"
        :owner "scope-mechanism 在 project-management"))

    (module-egress
      (desc "会话查询 + 复盘 + 审计 + WS 流 + 内部消费")

      (reader mcp-conversation-query
        :tools "mission_conversation_query / mission_conversation_analyze"
        :actions "get / list / search"
        :reads "conversations + messages + turns + events + narrations"
        :code "daemon/src/handlers/comm/conversation.rs")

      (reader mcp-retrospective-view
        :tool "mission_retrospective_manage"
        :actions "get / list"
        :reads "retrospective_results")

      (reader mcp-audit
        :tool "mission_audit"
        :reads "conversation_tool_calls + conversation_events")

      (reader mcp-llm-trace
        :tool "mission_llm_trace"
        :reads "conversation_tool_calls (+ category system-support :: gemini_requests)")

      (reader frontend-conversation-stream
        :source "pillar 四 event_log / DB watch"
        :emits "新 messages / 新 narrations"
        :via "daemon/src/bus/ws_bridge.rs")

      (reader context-pipeline-history
        :code "crates/missiond-daemon/src/context/"
        :reads "最近 messages + narrations (拼 LLM prompt)"
        :purpose "daemon 内部, 每次 LLM 调用都触发")

      (reader harvester-for-kb
        :consumer "module kb-manager :: worker-experience-harvester"
        :reads "conversations"
        :writes-to "knowledge (via kb-manager ingress)"
        :note "跨模块链路: conversations 是 KB 的语料库源头"))

    (module-tables-owned
      (desc "本模块独占 14 张表")
      (tables conversations conversation_messages conversation_turns conversation_events
              conversation_tool_calls conversation_topic_vectors conversation_labels
              message_embeddings message_embedding_skips message_narrations narration_cursors
              message_translations message_labels
              retrospective_results)
      (count 14)
      (removed-in-v0.4-revision "image_descriptions 挪到 category system-support (独立图片缓存, 无外键, 非 conversation-scoped)")
      (non-db-forms-owned
        (jsonl-source "~/.claude/projects/{encoded}/*.jsonl (see non-db-forms :: external-source-streams)"))))


  ;; ═════════════════════════════════════════════════════════════
  ;;  分类: System Support (系统支持层)
  ;;  非业务语义的基础表 — 被所有模块使用或支撑
  ;; ═════════════════════════════════════════════════════════════
  (category system-support
    (desc "系统级基础表 — 20 张, 无独立业务语义, 支撑全局运行")
    (maturity "基础层 — 结构稳定, 部分 legacy 待清理")

    (component global-observability
      (desc "跨项目的观测 / 审计 / 成本追踪 — 不按项目分")
      :note "v0.4.1: system_timeline 已移除 — event_log (pillar 四) 作为 timeline SSOT, 直接服务 mission_timeline / WS timeline-stream (经 projection)"
      (table incidents
        :purpose "告警/异常聚合"
        :writer "pillar 四 :: incident-writer"
        :dedup "dedupe_key + 时间窗口"
        :scoping-candidate true)
      (table token_usage_ledger
        :purpose "LLM 调用成本追踪"
        :writer "pillar 四 :: token-usage-writer"
        :aggregations "by model / slot / conversation / time"
        :scoping-candidate true)
      (table gemini_requests
        :purpose "Gemini API 调用性能日志"
        :writer "pillar 二 2.3 :: gemini-logger")
      (table gemini_file_uploads
        :purpose "Gemini 文件上传缓存 (只存引用)"
        :actual-storage "Gemini 服务端")
      (table router_chat_archive
        :purpose "Router 聊天历史归档"))

    (component infrastructure
      (desc "daemon 级基础状态 + 凭据 + 游标")
      (tables daemon_state credentials
              reconcile_watermarks gemini_cli_watermarks watcher_cursors consumer_watermarks
              backfill_progress backfill_failures)
      :note "游标类表是各种增量处理的进度记录")

    (component compute-runtime
      (desc "slot / 任务的运行时状态 — 本应归 pillar 二 worker")
      (tables slot_sessions slot_tasks dynamic_slots)
      :owner-alt "pillar 二 worker"
      :note "v0.4 暂留, 待与 pillar 二 协调后决定归属")

    (component legacy-tables
      (desc "老版 schema 疑似不再活跃")
      (tables tasks inbox events)
      :action-needed "实地确认使用情况; 若确认 deprecated 则 drop")

    (component vision-assets
      (desc "图片描述缓存 — 独立无外键, 按 image_hash 去重")
      (table image_descriptions
        :schema "image_hash / media_type / description / char_count / created_at"
        :writer "pillar 二 workers/codex/vision_worker.rs :: save_image_description"
        :reader "message enrichment 时按 hash 查询"
        :note "非 conversation-scoped (无 session_id FK); 是全局图片 → 文本 cache"))

    (module-tables-owned
      (desc "此分类持有 20 张表 — 待各模块/pillar 进一步认领以减少 system-support 规模")
      (count 20)
      (added-in-v0.4-revision "image_descriptions 从 conversation-logs 迁入 (该表无 FK, 非会话附属)")
      (removed-in-v0.4.1-revision "system_timeline 合并进 pillar 四 event_log (SSOT); timeline-writer 订阅者 + mission_timeline 改读 event_log")))


  ;; ═════════════════════════════════════════════════════════════
  ;;  横切能力 (Cross-Cutting)
  ;; ═════════════════════════════════════════════════════════════
  (cross-cutting
    (desc "贯穿 4 个模块 + system-support 的基础能力")

    (capability db-trait-abstraction
      (desc "MissionStore 超 trait 聚合 13 领域 store")
      :trait "MissionStore"
      :code "crates/missiond-core/src/db/traits.rs (~750 行)"
      :stores 13
      :invariant "其他 crate 只依赖 trait 不依赖实现"
      (impl pg-store :target "crates/missiond-core/src/db/pg_*/" :status production)
      (impl sqlite-store :status deprecated))

    (capability retention-policy
      (desc "按表粒度的保留/清理规则")
      :rules ("event_log: 30 天常规 / 3 天 ephemeral (pillar 四 SSOT) — 取代原 system_timeline 7 天"
              "conversation_messages/events: append-only 无清理"
              "knowledge: append-only, access_count 手动归档"
              "incidents: 粗粒度按时间 DELETE"
              "tool_calls: append-only")
      :code "daemon/src/bus/retention_cron.rs + event/lifecycle/retention.rs (pillar 四)")

    (capability migrations-runner
      (desc "schema 演进 — daemon 启动 phase 1 自动跑")
      :code "crates/missiond-core/migrations/"
      :count 20
      :automation "sqlx::migrate! 编译期检查 + 运行期执行"))


  ;; ═════════════════════════════════════════════════════════════
  ;;  跨分类注记 — 演进路径 + 开放问题
  ;; ═════════════════════════════════════════════════════════════
  (cross-cutting-notes

    (migration-log
      "v0.4 (2026-04-19 session):"
      "(1) 抽 kb-manager 模块: 从 system-support 划 9 张表 (knowledge + 4 kb_* + 4 ast/beacon)"
      "(2) 抽 conversation-logs 模块: 从 system-support 划 14 张表 (conversations + 10 派生 + retrospective_results)"
      "(3) project-management 吸收 skills (4 表) + specs (4 表: intent/plan/workflow/user_intents)"
      "(4) project-specs category 并入 project-management (移除)"
      "(5) board 补 owned-tables 4 张 (含新增 prompt_snapshots)"
      "(6) system-support 从 ~45 张表瘦身到 21 张"
      "(7) 每个模块加 :module-tables-owned 显式声明 ownership"
      "v0.4 第二轮修正 (基于代码调查):"
      "(A) kb-manager: 移除 code-prefetch + xjpcode-briefing (一个是 reader 一个写文件系统)"
      "(B) kb-manager: 修正 kb-access-audit writer 实际在 context_pipeline.rs"
      "(C) prompt_snapshots: kb-manager → board (PK=task_id, autopilot 写)"
      "(D) image_descriptions: conversation-logs → system-support (独立无 FK)"
      "(E) embedding-worker: 标记为 cross-module writer (写两个模块的表)"
      "(F) spec-db-sync: 明确标注为 UNIMPLEMENTED (intent/plan/workflow 是 dead schema)"
      "(G) retrospective_results: 标注 PK=session_id 可经 FK 推断 project_id"
      "v0.4.1 (2026-04-19 后续):"
      "(H) SSOT 合并: system_timeline 从 category system-support 移除"
      "    pillar 四 升级到 v1.3.0, 正式锁定 event_log = timeline SSOT"
      "    UI readers (mission_timeline + WS timeline-stream) 目标改读 event_log (via projection)"
      "    代码 cutover (drop 表 + 移除 timeline-writer + 迁 reader + FTS 索引) 待后续执行"
      "    总表数 61 → 60"
      "v0.4.2 (2026-04-19 — 4 模块 in/core/out 结构 harmonization):"
      "(I) board 模块: 3 section 补 (desc) + core 从纯 (component) 改用 (path/plumbing/helper) 三分法"
      "    主路径: task-queue-lifecycle (serves goal-1+2)"
      "    plumbing: data-model / state-machine / core-operations / events-emitted"
      "    helper: task-notes / agent-questions / prompt-snapshot"
      "(J) kb-manager 模块: core 从 6 component + 2 plumbing 改为 2 path + 5 plumbing + 2 helper"
      "    path: kb-semantic-recall (goal-1) / kb-code-awareness (goal-2)"
      "    helper: access-audit / operation-queue (serves goal-3)"
      "(K) conversation-logs 模块: core 从 10 component 改为 3 path + 5 plumbing (原始层) + 5 helper (派生层)"
      "    path: pty-to-structured-db (goal-1) / multi-engine-dispatch (goal-2) / derived-analysis-layer (goal-3)"
      "    plumbing: 原始 5 表 (session/raw/turn/events/tool_calls)"
      "    helper: 派生 5 类 (vectors/narration/translation/labels/retrospective)"
      "(L) 4 模块语义标签统一: (path) 主路径服务 primary-goal / (plumbing) 共用基础 / (helper) 附属"
      "    与 project-management 风格完全对齐, 每个 path 条目标 :serves goal-N"
      "v0.4.3 (2026-04-19 — CLAUDE.md 分层):"
      "(M) 全局 ~/.claude/CLAUDE.md + manager 从 memory pillar non-db-forms 移到 pillar 五 intent-layer"
      "    rationale: 全局 CLAUDE.md 是'系统如何被指挥'的元层声明, 非业务记忆"
      "(N) 项目级 <project>/CLAUDE.md 留在 memory pillar, 新增 project-management :: helper project-claudemd-manager"
      "    明确 per-project CLAUDE.md 的读/写/reload 归属"
      "v0.4.4 (2026-04-19 — 动作/指令 specs 迁出):"
      "(O) intent / plan / workflow / user_intents 4 DB 表从 project-management 迁到 pillar 五 intent-layer"
      "    rationale: 这些是 action/instruction 规约 (元层), 非项目 factual data"
      "(P) .missiond/v2/intent.lisp + intent-*.lisp + workflows/*.lisp 从 non-db-forms :: lisp-spec-files 迁出"
      "    rationale: system-level lisp + 方法论模板都是 action/instruction 层"
      "(Q) .missiond/flows/*.yaml 从 non-db-forms :: yaml-flow-templates 迁到 pillar 五"
      "    rationale: flow 模板是 action/instruction"
      "(R) project-management goal-1 改: 从'mcp-specs-precision'变'per-project-code-snapshot-access'"
      "    path project-specs-access → path project-code-snapshot (只访问 per-project intent.lisp FILE, 非 DB specs)"
      "    lisp-survey-worker writer 明确只写 FILE"
      "(S) memory pillar 保留'描述各个项目代码真实状态'的 per-project .missiond/intent.lisp (唯一 lisp-spec 形式)"
      "    memory pillar ownership: 5+4+9+14+20 = 52 张 (从 v0.4.3 的 56 下降 4)")

    (ownership-summary
      (module-project-management   5 "projects + 4 skills (specs 4 张迁走)")
      (module-board                4 "board_tasks + board_task_notes + agent_questions + prompt_snapshots")
      (module-kb-manager           9 "knowledge + 4 kb_* + 4 ast/beacon")
      (module-conversation-logs   14 "conversations + 10 conv/message 派生 + retrospective_results")
      (pillar-four-event-bus       4 "event_log (also SSOT for timeline v1.3.0+) + event_subscriptions + blob_storage + dlq")
      (pillar-five-intent-layer    4 "intent + plan + workflow + user_intents (v0.4.4 迁入, action/instruction 层)")
      (category-system-support    20 "observability + image_descriptions + infrastructure + compute-runtime + legacy")
      (total 60)
      (memory-pillar-subtotal 52 "memory 管 5+4+9+14+20 = 52 张 (v0.4.4 从 56 下降 4)")
      (total-delta-from-v0.4.3 "-4 in memory (specs 迁 pillar 五, 不减总量)"))

    (pending-actions
      "A. 给 candidates-for-promotion 中的高价值表加 project_id (token_usage / prompt_snapshots / specs / skills)"
      "B. 确认 legacy tables (tasks / inbox / events) 是否可 drop"
      "C. 与 pillar 二 协调 compute-runtime (slot_sessions/tasks/dynamic_slots) 归属"
      "D. 实现 spec-db-sync (intent.lisp 文件 ↔ intent DB 表)"
      "E. 派 agent 验证每个模块 owned-tables 在实际代码中是否被正确隔离 (不被其他 pillar 越界读写)")

    (design-rationale
      "v0.4 核心变化: 让每个业务模块承担自己表的所有权声明;"
      "不再把表都堆到 system-support; "
      "ownership 从隐含 (通过 :code 字段推断) 变成显式 ((module-tables-owned ...) 块)."
      "这使得其他 pillar / module 一眼能看出'这张表归谁管', 减少越界读写可能."))

) ;; end intent memory
