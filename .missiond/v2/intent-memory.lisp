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
  (version "draft-v0.4.15")
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
    (v0.4.4 "action/instruction specs 迁 pillar 五: intent/plan/workflow/user_intents DB 表 + system-level intent*.lisp + workflows*.lisp + flows/*.yaml; memory 只留'项目代码真实状态'的 per-project intent.lisp")
    (v0.4.5 "workflow-lisp-templates + flow-yaml-templates 合并到单一 component workflows (:kind methodology | executable), 符合 Option B 设计")
    (v0.4.6 "embedding 契约 SSOT: cross-cutting 新增 capability embedding-storage-governance, 5 处散落描述改用 cross-ref; 确认 5 张承载表 (含 skill_topics / ast_nodes) 及 message_embeddings 的 halfvec 特殊性")
    (v0.4.7 "board 模块按'memory=库'原则精简: autopilot + flow-engine-v2 计算逻辑移到 pillar 二 2.4 orchestration, board 只留 cross-ref")
    (v0.4.8 "board 新增 helper agent-execution-coordination: 从 intent-event-bus-execution.lisp 提取'并行 agent 共享内存层'模式, 6 slots + storage/manager interface 声明")
    (v0.4.9 "conversation-logs 按 'memory=库' 精简: 10 worker 全改 cross-ref + 新增 writer-removed 墓志铭 (worker-briefing 已在 v1.3.0 删) + 2 egress 消费者改 cross-ref + pillar 二 2.3 同步修正 briefing 删除 + 增 writes-to-memory 注记")
    (v0.4.10 "kb-manager 按 'memory=库' 精简: 5 worker + 1 context-pipeline writer → cross-ref; 2 egress compute 消费者 → cross-ref; core plumbing 的 :maintained-by 改 cross-ref; helper access-audit + operation-queue 加 :library-pov 分清接口 vs 时机")
    (v0.4.11 "project-management 按 'memory=库' 精简: lisp-survey-worker → cross-ref pillar 二 2.3; scope-propagation 从 ingress 'writer' 重构为 plumbing scope-mechanism 下的 write-propagation-convention (不是 writer 而是跨模块约定); vault-md-edit 加 :library-pov")
    (v0.4.12 "野生逻辑清理 Phase 1: L1 修正 (agent-questions auto-unblock 实际已实现) + L1 移除摘要 (message_narrations/cursors 下线 + step-narrator 删) + L2 TBD 修正 (kb_operation_queue 有 consumer / message_embedding_skips 是 audit trail) + L2 设计 (project-claudemd / agent-execution-coordination / prompt-snapshot 三个 MCP tool interface)")
    (v0.4.13 "Phase 2 第一步: 新增 module llm-support (3 表: gemini_requests / gemini_file_uploads / token_usage_ledger, 从 category system-support 分离); 调查确认 3 writer 实际路径 (token 纠正为 message_handler.rs, 不是 gateway); 确认 observability-consumer 存在 (timeline.rs enrichment); cross-module-trait-sharing 诚实披露 ObservabilityStore 跨模块使用")
    (v0.4.14 "Phase 2 第二步: 新增 module slot-support (3 表: slot_sessions / slot_tasks / dynamic_slots, 从 category system-support :: compute-runtime 分离); 校正 slot_tasks 实际语义为 learning engine 的 AI 任务 (extraction + decision), 不是通用 slot 任务; cross-module-trait-sharing 披露 SlotStore 跨模块 (daemon_state + legacy tasks/inbox/events)")
    (v0.4.15 "Phase 2 第三步: category system-support upgrade 为 module (14 张表 = 10 active + 4 legacy). 10 active 分: 观测 3 + infra 游标 4 + backfill 2 + daemon_state 1. legacy-zone 4 张 drop 决策: tasks keep / inbox deprecate / events drop / credentials drop (v0.4.15 新发现 dead schema). 关键校正: backfill_* 不是一次性迁移, 是 embedding-worker 持续 phased 作业; router_chat_archive 是 side-table, 主数据在 conversations (chat_type='router_chat'); 每 writer 定位到具体代码行"))
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
      (desc "pgvector 二进制 embedding 列 — 详见 cross-cutting :: embedding-storage-governance")
      :governance-ssot "cross-cutting :: capability embedding-storage-governance (v0.4.6+)"
      :quick-facts "512 dim / 5 承载表 / qwen3 via sonnet-gateway / HNSW cosine / 禁止降级兜底")

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
      (desc "MCP 库写入 API + lisp-survey-worker cross-ref + vault 用户编辑")
      :principle "memory = 库. 本模块只列'谁来写我的表'; worker 算法在 pillar 二 2.3"

      ;; ── MCP 库写入 API ──
      (writer mcp-project-mutation
        :tools  "mission_project init / sync / set_active / vault_sync / import_universe / survey"
        :writes "projects"
        :code   "daemon/src/handlers/knowledge/project.rs")

      (writer mcp-skill-mutation
        :tools  "mission_skill_mutate"
        :writes "skill_topics / skill_blocks / skill_versions / skill_executions"
        :code   "daemon/src/handlers/knowledge/skill.rs")

      ;; ── 外部 worker 驱动 (cross-ref) ──
      (writer lisp-survey-worker-snapshot
        :cross-ref "pillar 二 2.3 :: workers/sonnet/lisp_survey_worker.rs"
        :writes    "<project>/.missiond/intent.lisp (per-project 代码快照 FILE, 非 DB)"
        :library-pov "库不管 FILE 的存在, 只在 projects.intent_path 存路径指针; 扫描 + 写文件策略在 worker"
        :not-writes "DB intent/plan/workflow 表 — 这些在 pillar 五 intent-layer 不在本模块")

      ;; ── 外部 actor (用户 / Claude Code 直接编辑文件) ──
      (writer vault-md-edit
        :source "用户手动 / Claude Code Edit tool"
        :writes "~/.claude/projects/{encoded}/memory/*.md"
        :kind   "文件系统操作, 不走 DB"
        :library-pov "库侧无直接写入; 通过 helper project-memories-vault 读取文件"))

    ;; ── 作用域传播机制 (非 ingress writer, 是跨模块约定) ──
    ;; 移到 module-core plumbing scope-mechanism 下描述更合适 (v0.4.11 已整合)

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
        (desc "project_id 列 + 查询规则 + 跨模块 writer 传播约定")
        :rule "WHERE project_id = $X OR IS NULL"
        :semantics "NULL = 全局共享; 非 NULL = 项目私有"
        :applies-to "4 张实际有列: projects / knowledge / conversations / board_tasks"
        :candidates "~15 张可升级 (见 scoping-index :: candidates-for-promotion)"
        :see-also "顶层 scoping-index / table-catalog"

        (write-propagation-convention
          (desc "其他模块 writer 插入 scoped 表时的 project_id 填充约定")
          :resolve-via "ProjectRegistry::resolve(cwd)"
          (propagators
            "conversation-logger → conversations.project_id (inferred from PTY path)"
            "mcp-kb-mutation → knowledge.project_id (explicit param 或 resolve from cwd)"
            "mcp-board-lifecycle → board_tasks.project_id (explicit param 或 resolve from cwd)"
            "retro-worker → retrospective_results (inherited 继承 session_id → conversation)")
          :note "本约定是 library-side invariant: 所有 scoped 表写入必经 resolve; worker 实现在各自模块"))

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
        :cross-ref "全局 ~/.claude/CLAUDE.md 在 pillar 五 intent-layer :: global-claudemd (非本模块)"

        (mcp-tool-design mission_project_claudemd
          :status "🚧 设计完成, 待实现"
          (actions
            (read
              :args "project_id? (default: resolve from cwd)"
              :returns "markdown content + last_modified + size"
              :purpose "Agent 能按需读取项目 CLAUDE.md 而不是等下次会话启动")
            (edit
              :args "project_id? + new_content (整体替换) 或 patch (diff 格式)"
              :writes "<project>/CLAUDE.md"
              :side-effect "发 ProjectEvent::ClaudemdUpdated 到 event-bus → Claude Code 下次加载时读新版"
              :invariant "保留 git history — 不覆盖历史 commit, 新内容走正常文件 write")
            (reload
              :args "project_id?"
              :purpose "显式触发 Claude Code 重读 CLAUDE.md (如果在同一会话内想应用新内容)"
              :note "实际 reload 语义依赖 Claude Code 客户端支持; 当前仅发事件"))
          :implementation-plan "crates/missiond-daemon/src/handlers/knowledge/project_claudemd.rs (新文件)"
          :invariant "path-traversal 防护 (同 project_memory_vault.rs)"))

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
      (desc "任务创建 + 问题 API (库的写入口) + 外部 engine 驱动")
      :principle "memory = 库. 本模块只列'谁来写我的表'; engine 内部逻辑 (tick/决策/recovery) 见 pillar 二"

      ;; ── MCP 写入 API (库的直接写入面) ──
      (writer mcp-board-lifecycle
        :tools "mission_board_create / update / claim / decompose / retry / note_add / delete"
        :count 7
        :writes "board_tasks / board_task_notes"
        :code "daemon/src/handlers/knowledge/board.rs")

      (writer mcp-question-lifecycle
        :tool "mission_question (独立 MCP 工具, 非 board 工具面)"
        :writes "agent_questions (+ 自动 UPDATE 关联 board_tasks.status='blocked')"
        :code "daemon/src/handlers/.../question.rs")

      ;; ── 外部 engine 驱动的写入 (计算逻辑不在库内) ──
      (writer autopilot
        :cross-ref "pillar 二 2.4 orchestration :: component autopilot"
        :writes "board_tasks (CAS claim / status 推进 / lease 回收) + prompt_snapshots (task 执行存档)"
        :library-pov "库只暴露 BoardStore 原子操作 + prompt_snapshots schema; tick / 决策 / recovery 时机 由 engine 管")

      (writer flow-engine-v2
        :cross-ref "pillar 二 2.4 orchestration :: component flow-engine-v2"
        :writes "board_tasks.flow_context (每节点 persist)"
        :library-pov "库只暴露 update_board_task 接口 + flow_context JSONB 列; 节点执行顺序 / 变量插值 由 engine 管"))

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
        :lease-column "lease_expires_at (超期判定字段)"
        :recovery-interface "BoardStore::recover_stale_running_tasks() 提供强制 reset 接口"
        :recovery-scheduling "何时调用此接口由 pillar 二 2.4 :: autopilot 决定 — 库只暴露接口"
        :note "库只定义 FSM + 原子性; engine 负责'何时推进'")

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
        :answer-side-effect "answer_agent_question() 在全部 pending 问题解决时, CAS UPDATE board_tasks SET status='open' WHERE status='blocked'"
        :event-signal "发 QuestionEvent::Resolved 到 event-bus"
        :code "crates/missiond-core/src/db/question.rs :: create_question / answer_agent_question"
        :mcp "mission_question (独立 MCP 工具, 非 board 工具面)")

      (helper prompt-snapshot
        :table "prompt_snapshots"
        :schema "task_id PRIMARY KEY / prompt / cited_kb_ids / category / task_outcome / created_at"
        :purpose "task 执行时的 prompt 快照 + KB 引用审计"
        :writer "autopilot.rs :: save_prompt_snapshot"
        :cross-module-note "cited_kb_ids 关联 kb-manager :: knowledge; 但 PK 是 task_id 故归 board"

        (mcp-tool-design mission_prompt_snapshot
          :status "🚧 设计完成, 待实现"
          (actions
            (get
              :args "task_id"
              :returns "prompt + cited_kb_ids + category + task_outcome"
              :purpose "单 task 的 prompt 回放")
            (list
              :args "category? / time-range? / task_outcome? (filter) + limit"
              :returns "Vec<PromptSnapshotSummary>"
              :purpose "批量浏览 prompt 调优候选")
            (analyze
              :args "category OR time-range"
              :returns "cited-kb 频次统计 + task_outcome 分布 + prompt 长度分布"
              :purpose "prompt 工程洞察: 哪些 KB 被高频引用 / 哪种 prompt 模式更成功")
            (diff
              :args "task_id_a + task_id_b"
              :returns "两次 prompt 的文本 diff + KB 引用差"
              :purpose "对比两次相似 task 的 prompt 差异, 找调优方向"))
          :implementation-plan "crates/missiond-daemon/src/handlers/knowledge/prompt_snapshot.rs (新文件)"
          :invariant "只读工具 — 不改已存的 snapshot"))

      ;; ── 并行 agent 协作的共享内存层 (从 intent-event-bus-execution.lisp 提取的模式) ──
      (helper agent-execution-coordination
        (desc "并行 agent 协作的共享内存层 — 多 agent 同时工作时的冲突避免 / 决策协调 / 进度追踪")
        :rationale "事件总线重构 (v1.0→v1.3.0) 期间 16+ agent 并行工作, 此模式证明有效; 需作为 board 通用能力管起来"
        :pilot-instance ".missiond/v2/intent-event-bus-execution.lisp (D001-D016 deviations + DC001-DC049 decisions + I001-I010 issues 都在此)"

        (shared-memory-slots
          (phase-tracker "每个 agent 当前处于哪个 phase + 完成标记 — 避免重复执行")
          (claims        "谁锁定了哪个文件 / 模块 — 避免并发写冲突")
          (deviations    "意图(frozen lisp)与实际(code)的差异留痕 — 格式 D<NNN> + reason + approved-by")
          (decisions     "决策日志 — 格式 DC<NNN> + context + rationale + decided-at")
          (completions   "phase 完成清单 — 断点续跑基础")
          (issues        "阻塞 / 未决问题 — 格式 I<NNN> + severity + resolution"))

        (storage
          :current-format "Lisp 文件约定: .missiond/v2/<name>-execution.lisp"
          :pairing "每个 frozen 设计 lisp 有配套 execution lisp (e.g. intent-event-bus.lisp ↔ intent-event-bus-execution.lisp)"
          :file-governance-ref "frozen lisp 的 (file-governance) 块指向 :companion-log"
          :why-file "人类可读 + git 版本化 + agent 跨 session 持久"
          :db-option "未来可选 execution_contexts + execution_entries 2 表 (JSONB entries), 暂不做 — file 够用")

        (manager-interface
          :status "🚧 设计完成, 待实现 — 脱离 'agent 直接 Edit lisp' 的易冲突模式"
          :implementation-plan "crates/missiond-daemon/src/handlers/knowledge/agent_execution.rs (新文件)"

          (mcp-tool-design mission_execution
            (action claim
              :args "execution_id (e.g. 'intent-event-bus') + phase + claimer_name + scope (files / modules)"
              :writes "execution lisp 的 (claims) section 加一条"
              :atomicity "写入前检查 scope 是否已被别人 claim → 冲突返 ConflictError"
              :purpose "防并发修改同一文件")
            (action release
              :args "execution_id + claimer_name + claim_id"
              :writes "execution lisp 的 (claims) 移除 entry"
              :purpose "工作完成释放 claim")
            (action decide
              :args "execution_id + context + rationale + decided-by"
              :writes "execution lisp 的 (decisions) 新增 DC<NNN>"
              :id-allocation "自动分配下一个 DC id"
              :purpose "记录决策日志")
            (action deviate
              :args "execution_id + lisp-said + actually-did + reason + approved-by"
              :writes "execution lisp 的 (deviations) 新增 D<NNN>"
              :purpose "frozen lisp 与实际实现的偏差留痕")
            (action complete
              :args "execution_id + phase + agent_name + summary?"
              :writes "execution lisp 的 (completions) 加条 + phase-tracker 标完成"
              :purpose "标 phase 完成, 支持断点续跑")
            (action issue
              :args "execution_id + severity + desc + resolution?"
              :writes "execution lisp 的 (issues) 新增 I<NNN>"
              :purpose "记录阻塞/未决问题")
            (action status
              :args "execution_id"
              :returns "phase-tracker + active-claims + recent-decisions + open-issues"
              :purpose "Agent 启动时先查状态, 避免重复劳动"))

          :invariant-1 "所有写操作对 lisp 文件原子化 (整体 read → modify → write, 或 write-lock)"
          :invariant-2 "paren balance 写前写后校验"
          :invariant-3 "ID 分配避免冲突 (D<NNN> / DC<NNN> / I<NNN>)")

        (linked-concepts
          :methodology "pillar 五 :: workflows :kind methodology (e.g. bus-refactor.lisp) — 可复用方法论模板"
          :execution "本 helper 管的 — per-run 的 agent 协作实例"
          :distinction "methodology = '怎么做'的说明书 (reusable); execution = '这次做的过程' (per-run)"
          :future-board-linkage "可能把 phase → board_task 映射, execution 变成 board_tasks 的 flow_context 超集")

        (design-rationale
          "归 board 因为: (1) board 是任务编排中心 (2) 并行 agent = 多 task 协作 (3) 每个 phase 可视为 board_task 的 refactor-scoped 子结构"
          "不归 pillar 五 因为: pillar 五 是'应该做什么'的规约 (prescriptive); 本 helper 是'正在做什么'的实况 (operational state)")))

    (module-egress
      (desc "MCP 查询 + 前端 WS 流 — 库对外的读出口")
      :principle "autopilot 的 tick-scan 是 engine 内部自读自写闭环 (见 pillar 二 2.4 :: autopilot), 非库的独立 reader"

      (reader mcp-board-query
        :tool "mission_board_query"
        :actions "list / get / search / summary / clear_done")

      (reader frontend-board-stream
        :source "pillar 四 event_log subscribe BoardEvent"
        :emits "BoardTask* 事件 → 前端实时更新"
        :via "daemon/src/bus/ws_bridge.rs"))

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
      (desc "MCP 库写入 API + 外部 worker 驱动的写入 + daemon 内部审计写入")
      :principle "memory = 库. 本模块只列'谁来写我的表'; worker 的 tick / 算法 / 错误恢复见 pillar 二 2.3"

      ;; ── MCP 库写入 API ──
      (writer mcp-kb-mutation
        :tools  "mission_kb_mutate / mission_kb_remember / mission_kb_batch_set_project"
        :writes "knowledge (+ kb_operation_queue async)"
        :code   "daemon/src/handlers/knowledge/kb.rs")

      ;; ── 外部 worker 驱动的写入 (cross-ref pillar 二 2.3) ──
      (writer worker-arch-maintenance
        :cross-ref "pillar 二 2.3 :: workers/sonnet/arch_maintenance_worker.rs"
        :writes    "knowledge (category=architecture)"
        :library-pov "库暴露 KnowledgeStore write 接口 + category 约束; 扫描触发算法在 worker")

      (writer worker-experience-harvester
        :cross-ref "pillar 二 2.3 :: workers/local/experience_harvester.rs"
        :writes    "knowledge (category=bugfix/policy/memory)"
        :source    "module conversation-logs 的 conversations (跨模块 reader)"
        :library-pov "库暴露 write 接口; 挖掘策略 / 分类算法在 worker")

      (writer worker-tagger-chunker
        :cross-ref "pillar 二 2.3 :: workers/local/tagger_chunker.rs"
        :writes    "knowledge (分块 + 标签) + kb_ast_links"
        :library-pov "库暴露 knowledge + kb_ast_links 接口; 分块 + 打标算法在 worker")

      (writer worker-ast-sync
        :cross-ref "pillar 二 2.3 :: workers/local/ast_sync_worker.rs"
        :writes    "ast_nodes / ast_file_meta / beacons / beacon_nodes"
        :library-pov "库暴露 4 张代码索引表 + 增量 upsert 接口; 文件变更监听 + 解析在 worker")

      (writer worker-embedding-cross
        :cross-ref "pillar 二 2.3 :: workers/sonnet/embedding_worker.rs"
        :writes    "knowledge.embedding_vec + ast_nodes.embedding_vec"
        :cross-module "同 worker 也写 conv-logs (message_embeddings + topic_vectors) + project-mgmt (skill_topics)"
        :governance "cross-cutting :: capability embedding-storage-governance"
        :library-pov "库暴露 embedding_vec 列 + 类型约束; 生成调度 + provider 绑定在 worker")

      ;; ── daemon 内部 context-pipeline 审计写入 ──
      (writer context-pipeline-kb-audit
        :cross-ref "pillar 二 context-pipeline (daemon 内部, 非 worker)"
        :code "crates/missiond-daemon/src/context/context_pipeline.rs :: kb_log_co_access"
        :writes "kb_access_log"
        :library-pov "库暴露 kb_access_log append 接口; 何时写 (prefetch 后记录共访问) 由 context-pipeline 决定"))

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
        :flow  "代码索引 (ast + beacons) + kb_ast_links → mission_code_search 语义+结构并查"
        :entry "mission_code_search / mission_universe_graph"
        :tables "ast_nodes + ast_file_meta + beacons + beacon_nodes + kb_ast_links"
        :library-pov "库暴露 4 张代码索引表 + linkage 表; 文件变更监听 + AST 解析 → pillar 二 2.3 worker-ast-sync")

      ;; ── plumbing: KB 表族 ──
      (plumbing knowledge-store
        (desc "语义级记忆主表 — 40+ category, 每条独立知识片段")
        :table "knowledge"
        :schema-cols "id / key / category / content / embedding_vec / project_id / tags / access_count / created_at / updated_at"
        :code "crates/missiond-core/src/db/knowledge.rs"
        :scoping "has project_id column (primary scoped)"
        :embedding "见 cross-cutting :: embedding-storage-governance")

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
        :consumed-by "mission_code_search / universe_graph / kb-ast-linkage"
        :scoping "global (候选 per-project)"
        :maintenance-cross-ref "pillar 二 2.3 :: worker-ast-sync (写入调度)")

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
        :scoping "inherited"
        :retention "append-only; 按时间 DELETE (待策略)"
        :writer-cross-ref "pillar 二 context-pipeline :: kb_log_co_access"
        :library-pov "库暴露 append + 按时间 DELETE 接口; 触发时机由 context-pipeline 决定")

      (helper operation-queue
        :serves goal-3
        (desc "KB 变更操作队列 — 记录计划 + 同步消费")
        :table "kb_operation_queue"
        :schema "id / plan_id / task_id / operation / target_keys / status / result / error / executed_at"
        :scoping "inherited"
        :library-pov "库暴露 enqueue/update/complete/summary 接口"
        :writers "kb_ops_save_plan (daemon/src/db/pg/knowledge.rs)"
        :consumer-inline "mission_kb_ops 'list' 动作直接读 pending 并同步执行 (crates/missiond-daemon/src/handlers/knowledge/kb.rs)"
        :consumer-completion "pty_event_worker + memory_scheduler 调 kb_ops_complete_by_task_id()"
        :stats "kb_ops_plan_summary() GROUP BY status"
        :note "⚠ 不是 async background consumer — 是 MCP handler inline 消费 (同步式)"))

    (module-egress
      (desc "MCP 库查询 API + 外部 compute 消费者 cross-ref")
      :principle "memory = 库. 直接暴露 MCP 读接口; daemon 内部 compute / worker 读取只列 cross-ref"

      ;; ── MCP 库读取 API ──
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
        :focus "代码语义 + 结构并查"
        :invokes "pillar 二 2.6 search-engines")

      (reader mcp-universe-graph
        :tool "mission_universe_graph"
        :reads "knowledge + knowledge_edges (跨项目)"
        :focus "实体 / 关系图生成")

      ;; ── 外部 compute 消费者 (cross-ref, 非库独立 reader) ──
      (reader context-pipeline-retrieval
        :cross-ref "pillar 二 context-pipeline (daemon 内部)"
        :reads "knowledge (向量 + 最近)"
        :purpose "为 LLM 调用拼 prompt"
        :library-pov "库暴露 search + retrieval 接口; token 预算 / 多源打分 由 pipeline 定"
        :note "记忆最密集的消费者 — 每次 LLM 调用都触发")

      (reader worker-code-prefetch
        :cross-ref "pillar 二 2.3 :: workers/local/code_prefetch.rs"
        :reads "ast_nodes / beacons"
        :purpose "AST 混合搜索引擎 (FTS5 + embedding RRF)"
        :library-pov "库暴露查询接口; RRF 融合算法在 worker"))

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
        :solution "embedding (向量) + translation (多语种) + labels (打标) + retrospective (复盘)"
        :benefit  "多层次派生视图, 服务不同消费场景"
        :history  "briefing-worker (v1.3.0 删) + step-narrator + message_narrations/cursors (v0.4.12 删); 摘要能力完全移除 — 用户决策不再需要"))

    (module-ingress
      (desc "MCP 库写入 API + 外部 worker 驱动的写入")
      :principle "memory = 库. 本模块只列'谁来写我的表'; worker 的 tick / 算法 / 错误恢复见 pillar 二 2.3"

      ;; ── MCP 库写入 API (库直接暴露的写入面) ──
      (writer mcp-conversation-reconcile
        :tools  "mission_conversation_reconcile / mission_conversation_analyze"
        :writes "conversations / conversation_messages"
        :code   "daemon/src/handlers/comm/conversation.rs")

      ;; ── 三引擎原始摄入 worker (cross-ref pillar 二 2.3 local) ──
      (writer worker-conversation-logger
        :cross-ref "pillar 二 2.3 :: workers/local/conversation_logger.rs"
        :writes    "conversations + conversation_messages + conversation_turns + conversation_events"
        :library-pov "库暴露 ConversationStore append 接口 + (session_id, turn_id) 幂等; tail JSONL 算法在 worker")

      (writer worker-codex-ingestion
        :cross-ref "pillar 二 2.3 :: workers/local/codex_ingestion_worker.rs"
        :writes    "conversations / conversation_messages (engine_type=codex)")

      (writer worker-gemini-logger
        :cross-ref "pillar 二 2.3 :: workers/local/gemini_logger.rs"
        :writes    "conversations / conversation_messages (engine_type=gemini)")

      (writer worker-gemini-reconcile
        :cross-ref "pillar 二 2.3 :: workers/local/gemini_reconcile_worker.rs"
        :writes    "conversations (对账校正)")

      (writer worker-conversation-organizer
        :cross-ref "pillar 二 2.3 :: workers/local/conversation_organizer.rs"
        :writes    "conversation_turns / conversation_tool_calls")

      ;; ── 派生分析 worker (cross-ref pillar 二 2.3 sonnet/codex) ──
      (writer worker-embedding
        :cross-ref "pillar 二 2.3 :: workers/sonnet/embedding_worker.rs"
        :writes    "message_embeddings (halfvec) / conversation_topic_vectors"
        :cross-module "同一 worker 也写 kb-manager (knowledge.embedding_vec / ast_nodes) + project-management (skill_topics)"
        :governance "cross-cutting :: capability embedding-storage-governance"
        :library-pov "库暴露 embedding_vec 列 + halfvec/vector 类型约束; 生成算法在 worker")

      ;; worker-step-narrator 已随 narration 移除 (v0.4.12), 见下方 writer-removed

      (writer worker-translation
        :cross-ref "pillar 二 2.3 :: workers/sonnet/translation_worker.rs"
        :writes    "message_translations")

      (writer worker-retro
        :cross-ref "pillar 二 2.3 :: workers/sonnet/retro_worker.rs"
        :writes    "retrospective_results"
        :trigger   "会话结束信号 / 手动触发 (具体时机由 worker 定)")

      ;; ── 已删除的 writer (code 已去, lisp 保留墓志铭) ──
      (writer-removed worker-briefing
        :removed-in "SSOT cutover v1.3.0 (commit 6789509)"
        :was "crates/missiond-daemon/src/workers/sonnet/briefing_worker.rs"
        :wrote "message_narrations + narration_cursors (LLM 生成每轮会话摘要)"
        :reason "update_timeline_summary 路径与 append-only event_log 不兼容 + 语义摘要功能按决策不再需要")

      (writer-removed worker-step-narrator
        :removed-in "v0.4.12 (2026-04-19, 连同 narration 表一起下线)"
        :was "crates/missiond-daemon/src/workers/codex/step_narrator.rs"
        :wrote "message_narrations (codex 专项步骤叙述)"
        :reason "随 message_narrations + narration_cursors 表一起下线 (用户决策: 摘要功能完全不再需要)"
        :code-action-needed "代码层需移除文件 + 取消 codex mod 引用 + 删 migration for message_narrations / narration_cursors tables"))

      ;; 注: vision_worker 写 image_descriptions, 但该表在 v0.4.4 迁到 category system-support
      ;; (独立图片缓存, 无 conversation FK). vision_worker 不是本模块 ingress.
      )

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
        (desc "消息 + 话题向量 — embedding 治理见 cross-cutting :: embedding-storage-governance")
        :tables (conversation_topic_vectors message_embeddings message_embedding_skips)
        :note "conversation_topic_vectors 是 vector, message_embeddings 是 halfvec (1KB 压缩); 不同 HNSW 参数见 governance"
        :generator "worker-embedding (sonnet)"
        :consumer "pillar 二 2.6 search-engines :: vector-hnsw")

      ;; helper narration 已在 v0.4.12 移除 (message_narrations + narration_cursors 连同 step-narrator 下线)
      ;; 墓志铭: 历史上由 briefing-worker (v1.3.0 删) + step-narrator (v0.4.12 删) 生成
      ;; 用户决策: 语义/步骤摘要功能不再需要

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
      (desc "MCP 库查询 API + 前端 WS 流 + 外部消费者 cross-ref")
      :principle "memory = 库. 直接暴露 MCP / WS 读接口; daemon 内部 compute 消费者只列 cross-ref"

      ;; ── MCP 库读取 API ──
      (reader mcp-conversation-query
        :tools "mission_conversation_query / mission_conversation_analyze"
        :actions "get / list / search"
        :reads "conversations + messages + turns + events"
        :code "daemon/src/handlers/comm/conversation.rs"
        :note "v0.4.12: 不再读 narrations (表下线)")

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

      ;; ── 前端 WS 流 ──
      (reader frontend-conversation-stream
        :source "pillar 四 event_log / DB watch"
        :emits "新 messages (v0.4.12: 不再推 narrations)"
        :via "daemon/src/bus/ws_bridge.rs")

      ;; ── 外部 compute 消费者 (cross-ref, 非库独立 reader) ──
      (reader context-pipeline-history
        :cross-ref "pillar 二 context-pipeline (daemon 内部)"
        :reads "最近 messages (v0.4.12: 不再读 narrations)"
        :purpose "每次 LLM 调用拼 prompt 时触发"
        :library-pov "库暴露 ConversationStore 查询接口; pipeline 策略 / token 预算在 daemon")

      (reader harvester-for-kb
        :cross-ref "module kb-manager :: worker-experience-harvester"
        :reads "conversations"
        :writes-to "knowledge (经 kb-manager ingress)"
        :note "跨模块链路: conversations 是 KB 语料库源头; 本条仅表明 '此 worker 读我的表'"))

    (module-tables-owned
      (desc "本模块独占 12 张表")
      (tables conversations conversation_messages conversation_turns conversation_events
              conversation_tool_calls conversation_topic_vectors conversation_labels
              message_embeddings message_embedding_skips
              message_translations message_labels
              retrospective_results)
      (count 12)
      (removed-in-v0.4-revision "image_descriptions 挪到 category system-support (独立图片缓存, 无外键, 非 conversation-scoped)")
      (removed-in-v0.4.12 "message_narrations + narration_cursors 下线 (连同 briefing-worker [v1.3.0 删] + step-narrator [v0.4.12 删]); 摘要功能完全移除, 需 drop migration")
      (non-db-forms-owned
        (jsonl-source "~/.claude/projects/{encoded}/*.jsonl (see non-db-forms :: external-source-streams)"))))


  ;; ═════════════════════════════════════════════════════════════
  ;;  Support 模块 5: LLM Support Manager
  ;;  对 LLM worker 的数据支持 — 请求日志 / 文件引用 / 成本追踪
  ;; ═════════════════════════════════════════════════════════════
  (module llm-support
    (desc "LLM 调用的观测数据层 — 每次请求 + 远程文件引用 + token 成本")
    (maturity "成熟 (3 张表均有 active 读写)")
    :migrated-from "v0.4.13: 从 category system-support :: component global-observability 分离 (用户决策: 独立 manager)"

    (primary-goals
      (goal-1 llm-call-observability
        :problem  "LLM 调用分散在多个 gateway (sonnet / gemini / codex / minimax) + 多处 worker, 成本 / 延迟 / 错误没有统一留痕"
        :solution "gemini_requests 表按每次调用存 caller / model / prompt_chars / duration_ms / status / error_msg"
        :benefit  "mission_llm_trace 支持按模型 / 时段 / 错误做聚合分析")
      (goal-2 cost-tracking
        :problem  "多模型混用下 token 花销需按 session × 模型 维度统计"
        :solution "token_usage_ledger: conversation_id × model × (input / cache_creation / cache_read / output) tokens"
        :benefit  "成本归因到会话 / 模型 / 时段; 未来按项目聚合 (scoping candidate)")
      (goal-3 remote-file-cache
        :problem  "Gemini File API 上传有 URI + TTL, 同文件重传浪费带宽"
        :solution "gemini_file_uploads 按 file_hash 去重, 存 file_uri + expires_at"
        :benefit  "同文件复用 URI; TTL 过期后自然失效"))

    (module-ingress
      (desc "LLM gateway + logger worker 写入 3 张表")
      :principle "memory = 库. 本模块只列'谁来写我的表'; gateway / worker 算法见 pillar 二 2.2 + 2.3"

      (writer worker-gemini-logger
        :cross-ref "pillar 二 2.3 :: workers/local/gemini_logger.rs"
        :mechanism "event-bus subscriber on LlmEvent; two-step persist:"
        :step-1    "gemini_log_insert_started (id + caller + model + prompt_chars + prompt_text)"
        :step-2    "gemini_log_update_completed (id + api_mode + response + duration_ms + status)"
        :writes    "gemini_requests"
        :startup   "开机清 7 天前旧日志 (gemini_log_cleanup)"
        :library-pov "库暴露 gemini_log_insert_started + _update_completed; event-bus 订阅 + 采样 / 清理 策略在 worker")

      (writer llm-gateways-file-cache
        :cross-ref "pillar 二 2.2 :: daemon/src/llm/gemini_file_api.rs"
        :writes    "gemini_file_uploads (file_hash 去重 upsert)"
        :api       "ObservabilityStore::gemini_file_cache_get / _put / _gc"
        :library-pov "库暴露 cache get/put/gc 三接口; 上传前查 cache miss 再调 Gemini API, TTL 决策在 gateway")

      (writer token-usage-accounting
        :cross-ref "daemon/src/infra/message_handler.rs:674 (PTY JSONL 摄入下游)"
        :trigger "conversation-logger 摄入时, 对含 usage 字段的消息调 insert_token_usage"
        :writes  "token_usage_ledger (每消息一条: conversation_id + slot_id + model + 4 个 tokens + message_id)"
        :api     "ObservabilityStore::insert_token_usage"
        :library-pov "库暴露 insert_token_usage + token_stats; 摄入侧决定 '何时写 / 挑哪些消息'"
        :correction-v0.4.13 "之前假设在 gateway wrapper / fanout subscriber 是错的; 实际是摄入层"))

    (module-core
      (desc "3 张 LLM 观测表 — schema + 语义; 统一通过 ObservabilityStore super-trait 访问")
      :trait "ObservabilityStore (crates/missiond-core/src/db/traits.rs:614) — 注: 此 trait 还管 incidents / watermark / labels, 跨 llm-support / system-support / conversation-logs 多模块"

      (plumbing llm-request-log
        (desc "每次 LLM 请求的日志项")
        :table "gemini_requests"
        :schema-cols "id / caller / session_id / api_mode / model / prompt_chars / response_chars / queue_wait_ms / duration_ms / retry_count / status / error_msg / prompt_text / response_text / created_at"
        :write-pattern "two-step (started + update_completed) — 见 ingress worker-gemini-logger"
        :trait-methods "gemini_log_insert_started / _update_completed / _get_content / _query / _stats / _cleanup"
        :retention "7 天 (gemini-logger 启动时 cleanup)"
        :scoping-candidate "加 project_id 可按项目统计 LLM 使用"
        :code "crates/missiond-core/src/db/gemini_log.rs + pg/observability.rs (PG impl)")

      (plumbing remote-file-cache
        (desc "Gemini File API 文件引用 + TTL")
        :table "gemini_file_uploads"
        :schema-cols "file_hash (PK) / file_uri / mime_type / expires_at"
        :trait-methods "gemini_file_cache_get / _put / _gc"
        :note "PG 只存引用, 实际文件在 Gemini 服务端; upsert on conflict (hash)"
        :code "crates/missiond-core/src/db/gemini_log.rs + pg/observability.rs")

      (plumbing token-ledger
        (desc "LLM token 成本按 会话 × 模型 聚合")
        :table "token_usage_ledger"
        :schema-cols "conversation_id / slot_id / slot_task_id / model / input_tokens / cache_creation_tokens / cache_read_tokens / output_tokens / message_id / created_at"
        :trait-methods "insert_token_usage / token_stats (group_by: conversation / slot / model)"
        :aggregations "by model / by session / by slot / time-series"
        :scoping-candidate "加 project_id 后可按项目做成本报表"
        :code "crates/missiond-core/src/db/incident.rs (声明) + pg/observability.rs (impl)"))

    (module-egress
      (desc "MCP 查询 LLM 观测数据")

      (reader mcp-llm-trace
        :tool    "mission_llm_trace"
        :reads   "gemini_requests"
        :api     "ObservabilityStore::gemini_log_query / _stats / _get_content"
        :code    "daemon/src/handlers/comm/question.rs :: handle (注: 与 mission_question / decision_stats / incident / gemini_auth 共用 question.rs handler)"
        :actions "list / by-session / by-model / by-time")

      (reader mcp-cost-report
        :status  "🚧 MCP 未实现, 但底层查询原语已有"
        :tool    "mission_cost_report"
        :reads   "token_usage_ledger"
        :api     "ObservabilityStore::token_stats(conversation_id, slot_id, since, group_by) 已实现"
        :purpose "token 花销报表 — by model / by conversation / time-series / 未来按项目"
        :implementation-effort "低 — 只需写 MCP handler 包装 token_stats 查询, 核心 SQL 已在 trait 里")

      (reader timeline-enrichment
        :cross-ref "daemon/src/handlers/comm/timeline.rs:125"
        :reads "gemini_requests (用于 enrich gemini_request_completed 事件的 timeline 展示)"
        :api "ObservabilityStore::gemini_log_get_content(request_id)"
        :purpose "Timeline UI/CLI 显示 LLM 调用事件时, 从 gemini_requests 拉完整 prompt/response 细节")

      (reader daemon-stats-counter
        :cross-ref "daemon/src/infra/daemon_stats.rs"
        :reads "gemini_requests 计数 (AtomicU64 in-memory counter, 不查 DB)"
        :purpose "daemon 运行时 observability 计数"
        :note "内存计数, 不是 DB reader, 列此仅说明 observability 面板数据源之一"))

    (module-tables-owned
      (desc "本模块独占 3 张表")
      (tables gemini_requests gemini_file_uploads token_usage_ledger)
      (count 3)
      (migrated-from "v0.4.11 category system-support :: component global-observability"))

    (cross-module-trait-sharing
      (desc "ObservabilityStore super-trait 横跨多模块, 本模块只用其中一部分")
      :shared-trait "ObservabilityStore (db/traits.rs:614)"
      :this-module-uses "gemini_log_* (6 方法) + gemini_file_cache_* (3 方法) + insert_token_usage / token_stats"
      :other-modules-use
        ("incident 方法 (insert_incident / has_recent_incident / list_incidents) → module system-support"
         "watermark 方法 (watermark_get / _advance_* / _list) → module system-support"
         "label 方法 (label_set / _get / _find_messages) → module conversation-logs"
         "router chat 方法 → module system-support")
      :design-note "trait 按实现文件切分, 不按模块语义切分. 这是 lisp ↔ code 的一处阻抗不匹配, 可接受."))


  ;; ═════════════════════════════════════════════════════════════
  ;;  Support 模块 6: Slot Support Manager
  ;;  对 slot (工位) 的数据支持 — session 绑定 + AI 任务 + 动态 slot 生命周期
  ;; ═════════════════════════════════════════════════════════════
  (module slot-support
    (desc "slot 运行时数据层 — slot↔session 绑定 + learning engine AI 任务 + 动态 slot 生命周期")
    (maturity "成熟 (3 张表均有 active 读写, 多 writer 多 reader)")
    :migrated-from "v0.4.13: 从 category system-support :: compute-runtime 分离 (用户决策: 独立 manager)"

    (primary-goals
      (goal-1 slot-session-binding
        :problem  "slot 和 PTY session 1:1 绑定, daemon 重启后需恢复; 多处 ingestion worker 都需 session→slot 反查"
        :solution "slot_sessions 表维护 slot_id (PK) ↔ session_id, 双向查询 (get_slot_session / get_slot_for_session)"
        :benefit  "daemon 重启后 binding 可恢复; reconcile workers 可精准对账")
      (goal-2 slot-ai-task-queue
        :problem  "learning engine 的 AI 任务 (extraction / decision) 需持久化进度 + 支持 reap stale"
        :solution "slot_tasks 表记录 task_type + status + prompt_summary + conversation_id + 时长"
        :benefit  "任务崩溃可恢复; 历史可查询 (mission_slot_history); 僵尸任务可自动回收"
        :correction-v0.4.13 "之前以为是通用 slot 内任务, 实际是 learning engine 专属 (extraction + decision)")
      (goal-3 dynamic-slot-lifecycle
        :problem  "某些 flow 需临时 slot + TTL + 可 extend; 重启/过期需清理"
        :solution "dynamic_slots 表管 (create / extend / terminate), 索引 (status, expires_at) 支撑 GC 扫描"
        :benefit  "支持 slot 树 (parent_slot_id); TTL 默认 14400s (4 小时)"))

    (module-ingress
      (desc "多路 writer — session 绑定 / AI 任务创建 / dynamic slot lifecycle")
      :principle "memory = 库. 本模块只列'谁来写我的表'; SlotManager 的 lifecycle 逻辑见 pillar 二 2.1"

      ;; ── slot_sessions writers (4 处) ──
      (writer slot-orchestrator-bind
        :cross-ref "pillar 二 2.1 :: daemon/src/slot_orchestrator/mod.rs:42"
        :writes    "slot_sessions (slot 启动时绑定 session)"
        :library-pov "库暴露 set_slot_session (upsert on conflict); orchestrator 决定何时绑定")

      (writer slot-env-bind
        :cross-ref "pillar 二 2.1 :: daemon/src/context/slot_env.rs:197"
        :writes    "slot_sessions (slot 环境构建时同步)")

      (writer ingestion-router-bind
        :cross-ref "daemon/src/infra/ingestion_router.rs:79,104"
        :writes    "slot_sessions (JSONL 摄入发现新 session 时更新)")

      ;; ── slot_tasks writers (learning engine 专属) ──
      (writer learning-engine-extraction
        :cross-ref "pillar 二 :: daemon/src/engine/learning_engine/extraction.rs (3 call sites)"
        :writes    "slot_tasks (task_type=extraction 类, 3 种场景)"
        :library-pov "库暴露 insert_slot_task + status 转换接口 (set_running / set_completed / set_failed); 触发 + 派发在 extraction engine")

      (writer learning-engine-decision
        :cross-ref "pillar 二 :: daemon/src/engine/learning_engine/decision_engine.rs:805"
        :writes    "slot_tasks (task_type=decision)")

      ;; ── dynamic_slots writers (MCP + 清理) ──
      (writer mcp-compute-slot
        :tools     "mission_compute_slot (actions: create / terminate / extend / list)"
        :code      "daemon/src/handlers/compute/compute_slot.rs"
        :writes    "dynamic_slots (create:216 / terminate:293,339 / extend:385)"
        :library-pov "库暴露 create/terminate/extend/list 接口 + (status, expires_at) 索引; MCP handler 做业务编排")

      (writer daemon-restart-cleanup
        :cross-ref "daemon/src/main.rs:297"
        :writes    "dynamic_slots (terminate on 'daemon_restart')"
        :library-pov "库暴露 terminate_dynamic_slot; 重启策略 (全部强制终止 active slot) 在 daemon bootstrap")

      (writer autopilot-ttl-gc
        :cross-ref "pillar 二 2.4 :: autopilot (daemon/src/engine/intent_engine/autopilot.rs:1357)"
        :writes    "dynamic_slots (terminate on 'ttl_expired')"
        :library-pov "库暴露 find_expired_dynamic_slots 扫描接口 + terminate; tick 频率 + 决策在 autopilot"))

    (module-core
      (desc "3 张 slot 运行时表 — schema + 语义; 统一通过 SlotStore super-trait 访问")
      :trait "SlotStore (crates/missiond-core/src/db/traits.rs:478) — 注: 此 trait 还管 daemon_state + legacy tasks/inbox/events, 跨 slot-support / system-support / legacy 多模块"

      (plumbing slot-sessions
        (desc "slot ↔ PTY session 的 1:1 绑定表")
        :table "slot_sessions"
        :schema-cols "slot_id (PK) / session_id NOT NULL / updated_at BIGINT"
        :write-pattern "upsert on conflict (slot_id) DO UPDATE"
        :trait-methods "get_slot_session / set_slot_session / delete_slot_session / get_all_slot_sessions / get_slot_for_session / cleanup_pty_placeholder"
        :bidirectional "slot→session (get_slot_session) + session→slot (get_slot_for_session, 用于 reconcile)"
        :code "crates/missiond-core/src/db/slot.rs:28")

      (plumbing slot-tasks
        (desc "learning engine AI 任务队列 (extraction + decision 两类)")
        :table "slot_tasks"
        :schema-cols "id (PK) / slot_id / task_type / status (default 'pending') / prompt_summary / source_sessions / output_count / created_at / started_at / completed_at / duration_ms / error / conversation_id"
        :indexes "3 个: (slot_id) / (task_type) / (created_at)"
        :state-machine "pending → running → completed / failed"
        :trait-methods "insert_slot_task / slot_task_set_running / _set_completed / _set_failed / list_slot_tasks / slot_task_stats / reap_stale_slot_tasks / find_stale_decision_tasks / cleanup_orphan_slot_tasks / last_completed_slot_task_at / get_running_slot_task"
        :note "task_type 实际 values 由 learning engine 决定 (extraction / decision 是已知两类)"
        :code "crates/missiond-core/src/db/slot.rs:96-137")

      (plumbing dynamic-slots
        (desc "按需创建的临时 slot + TTL + parent 继承")
        :table "dynamic_slots"
        :schema-cols "id (PK) / parent_slot_id / template / objective / config (JSON) / status (default 'active') / termination_reason / created_at / terminated_at / ttl_seconds (default 14400 = 4h) / expires_at / extend_count"
        :indexes "idx_dynamic_slots_active on (status, expires_at) — 支撑 find_expired 扫描"
        :structure "支持 slot 树 (parent_slot_id 可嵌套)"
        :ttl-strategy "ttl_seconds 默认 4 小时; 可 extend (extend_count 递增); expires_at < now() 进 find_expired_dynamic_slots"
        :trait-methods "create_dynamic_slot / get_dynamic_slot / list_dynamic_slots / count_active_dynamic_slots / terminate_dynamic_slot / extend_dynamic_slot / find_expired_dynamic_slots / find_expiring_dynamic_slots"
        :code "crates/missiond-core/src/db/dynamic_slot.rs"))

    (module-egress
      (desc "MCP 查询 + daemon 内部 (reconcile / startup / events_sync)")

      ;; ── MCP readers ──
      (reader mcp-slots
        :tool "mission_slots"
        :reads "slot_sessions + dynamic_slots"
        :code "daemon/src/handlers/sysinfra/misc.rs (与 mission_slot_history / mission_inbox 共用 handler)"
        :purpose "列当前 slot + session 绑定 + active dynamic slots")

      (reader mcp-slot-history
        :tool "mission_slot_history"
        :reads "slot_tasks"
        :code "daemon/src/handlers/sysinfra/misc.rs:349 (调 list_slot_tasks)"
        :purpose "slot 内 AI 任务历史 (含 pending / running / completed / failed)")

      (reader mcp-compute-slot-query
        :tool "mission_compute_slot (action=list)"
        :reads "dynamic_slots"
        :code "daemon/src/handlers/compute/compute_slot.rs:413 (调 list_dynamic_slots)")

      (reader mission-memory-task-trail
        :tool "mission_memory (附加 slot task 快照)"
        :code "daemon/src/handlers/knowledge/memory.rs:269"
        :reads "slot_tasks (按 slot_id 拉最近 10 条)"
        :purpose "项目 memory 查询时附带 slot 执行轨迹")

      ;; ── daemon 内部 readers ──
      (reader daemon-startup-recovery
        :cross-ref "daemon/src/main.rs:294,423"
        :reads "list_dynamic_slots('active') + get_all_slot_sessions"
        :purpose "daemon 启动时: (1) 扫 active 动态 slot 做 terminate 清理 (2) 恢复现有 slot↔session 映射")

      (reader reconcile-workers
        :cross-ref "daemon/src/workers/local/reconcile_worker.rs + gemini_reconcile_worker.rs"
        :reads "get_slot_for_session (session → slot 反查)"
        :purpose "对账 workers 发现 session 时反查 slot 做归属判断")

      (reader events-sync-lookup
        :cross-ref "daemon/src/events_sync.rs:948"
        :reads "get_slot_for_session"
        :purpose "PTY 事件同步时 session → slot 反查")

      (reader ingestion-router-lookup
        :cross-ref "daemon/src/infra/ingestion_router.rs:123"
        :reads "get_slot_for_session"
        :purpose "JSONL 摄入路由时反查")

      (reader session-util
        :cross-ref "daemon/src/infra/session_util.rs:25"
        :reads "get_all_slot_sessions"
        :purpose "infra 工具读全部绑定"))

    (module-tables-owned
      (desc "本模块独占 3 张表")
      (tables slot_sessions slot_tasks dynamic_slots)
      (count 3)
      (migrated-from "v0.4.11 category system-support :: compute-runtime component"))

    (cross-module-trait-sharing
      (desc "SlotStore super-trait 横跨多模块, 本模块只用其中一部分")
      :shared-trait "SlotStore (db/traits.rs:478)"
      :this-module-uses "slot_sessions 方法 (6) + slot_tasks 方法 (11) + dynamic_slots 方法 (8) = 25 方法"
      :other-modules-use
        ("daemon_state 方法 (daemon_state_get / _set) → module system-support"
         "generic tasks 方法 (insert_task / update_task / get_task / get_tasks_by_status / ack_completed_tasks / ...) → module system-support :: legacy-zone :: tasks"
         "inbox 方法 (insert_inbox_message / get_inbox_messages / mark_inbox_read) → module system-support :: legacy-zone :: inbox"
         "events (legacy) 方法 (insert_event / get_events_by_task) → module system-support :: legacy-zone :: events")
      :design-note "trait 按实现文件切分 (slot.rs + dynamic_slot.rs + task.rs) — 同 ObservabilityStore 的阻抗不匹配模式, 接受"))


  ;; ═════════════════════════════════════════════════════════════
  ;;  Support 模块 7: System Support Manager
  ;;  系统级基础表 — 观测 / infra 游标 / backfill / legacy zone
  ;; ═════════════════════════════════════════════════════════════
  (module system-support
    (desc "系统级基础数据支持 — 告警 / 路由归档 / vision 缓存 / 多种游标 / backfill / daemon_state + 4 张 legacy 表")
    (maturity "成熟 (10 张 active) + 4 张 legacy 待清理")
    :migrated-from "v0.4.15: 从 category 升级到 module (LLM 3 + slot 3 分离后的留存)"

    (primary-goals
      (goal-1 system-level-observability
        :problem  "告警 / router 归档 / vision 缓存 分散, 需统一留痕"
        :solution "3 张观测表: incidents (event-bus 集中) / router_chat_archive (压缩归档) / image_descriptions (按 hash 缓存)"
        :benefit  "跨项目 / 跨模块的系统可观测性")
      (goal-2 infra-state-continuity
        :problem  "各增量 worker / consumer 的游标 + embedding backfill 进度需持久化, daemon 重启不丢"
        :solution "7 张 infra 表: 4 watermark/cursor + 2 backfill + daemon_state KV"
        :benefit  "崩溃恢复基础 — 每 worker 从自己的游标继续")
      (goal-3 legacy-cleanup
        :problem  "4 张老版 schema 活跃度不一, 需逐个判决"
        :solution "legacy-zone 明确 4 表状态 (tasks keep / inbox deprecate / events drop / credentials drop 新发现)"
        :benefit  "下轮 migration 可定向 DROP"))

    (module-ingress
      (desc "观测 + infra + legacy 3 类写入")
      :principle "memory = 库. worker / handler / event-bus fanout 各管时机 + 业务"

      ;; ── 观测 writers (3) ──
      (writer aiops-incident-reactor
        :cross-ref "daemon/src/infra/aiops.rs:145 process_incident (写入点 192 / 338)"
        :trigger "event-bus IncidentEvent::Reported subscriber (bus/v2_subscribers.rs:98 v2_incident_reactor)"
        :writes "incidents (含 dedupe_key 去重)"
        :trait-methods "ObservabilityStore::insert_incident / has_recent_incident / list_incidents"
        :library-pov "库暴露 insert + dedup check; triage 逻辑在 aiops")

      (writer router-chat-archiver
        :cross-ref "daemon/src/handlers/comm/router_chat.rs + pg/observability.rs router_chat_clear 族"
        :trigger "router_chat_clear / _clear_by_task 等时机 — 归档被删除 / 压缩的旧消息"
        :writes "router_chat_archive (original_id / session_id / role / content / archive_reason)"
        :trait-methods "ObservabilityStore::router_chat_* (22 方法族)"
        :library-pov "库暴露 router_chat_* 方法族; archive 策略在 handler")

      (writer vision-worker
        :cross-ref "pillar 二 2.3 :: workers/codex/vision_worker.rs :: save_image_description"
        :writes "image_descriptions (upsert by image_hash)"
        :library-pov "库暴露按 hash upsert; vision LLM 调用 + 去重决策在 worker")

      ;; ── infra 游标 + backfill writers (7) ──
      (writer watcher-cursor-writers
        :cross-ref "多个 file-watcher 类 worker (具体 worker 待补 — 方法签名是 batch upsert)"
        :writes "watcher_cursors (file_path PK / byte_offset / session_id)"
        :trait-methods "ObservabilityStore::load_watcher_cursors / upsert_watcher_cursors_batch / delete_watcher_cursor"
        :library-pov "库暴露 load/batch-upsert/delete; watcher 算法在 worker")

      (writer reconcile-worker-watermarks
        :cross-ref "pillar 二 2.3 :: workers/local/reconcile_worker.rs"
        :writes "reconcile_watermarks (jsonl_path PK / last_reconciled_size / last_reconciled_at)"
        :trait-methods "ObservabilityStore::get_reconcile_watermark / upsert_reconcile_watermark / get_all_reconcile_watermarks")

      (writer gemini-reconcile-watermarks
        :cross-ref "pillar 二 2.3 :: workers/local/gemini_reconcile_worker.rs"
        :writes "gemini_cli_watermarks (session_file PK / session_id / last_msg_count)"
        :trait-methods "ObservabilityStore::load_gemini_cursors / save_gemini_cursor")

      (writer consumer-watermarks-writers
        :cross-ref "多 consumer 通过 watermark_* API 更新 (具体 consumers 散在多 worker)"
        :writes "consumer_watermarks (consumer_name + session_id 复合 PK / last_processed_msg_id / extra)"
        :trait-methods "ObservabilityStore::watermark_get / _advance_time / _advance_msg_id / _advance_full / _list")

      (writer embedding-worker-backfill
        :cross-ref "pillar 二 2.3 :: workers/sonnet/embedding_worker.rs (写入点 1236 / 1322 / 1327 / 1343 / 1544 / 1597 / 1663 / 1912 / 1936 / 1953)"
        :writes "backfill_progress (phase 进度) + backfill_failures (重试记录)"
        :known-phases "conv_topic_vectors / conv_summary (session-level embedding backfill)"
        :trait-methods "MissionDB inherent: backfill_start_phase / _update_progress / _complete_phase / _record_failure / _retryable_failures / _retryable_failures_no_cooldown / _clear_failure / _get_phase / _list_phases"
        :correction-v0.4.15 "原以为是一次性迁移表 — 实际是 embedding-worker 持续的 phased 作业状态管理, active")

      (writer daemon-state-writer
        :cross-ref "daemon bootstrap + 各 state 更新处 (多处)"
        :writes "daemon_state (key PK / value / updated_at)"
        :trait-methods "SlotStore::daemon_state_get / _set"
        :note "trait 归 SlotStore (db/slot.rs 同文件), 实际用途是 daemon 全局 KV")

      ;; ── Legacy zone writers (2 active + 2 dead) ──
      (writer legacy-task-api-writers
        :cross-ref "crates/missiond-core/src/db/task.rs + db/pg/slot.rs (legacy v1 slot API 路径)"
        :writes "tasks (active) / inbox (轻度) / events (疑似 dead, 仅 legacy 写入)"
        :status "⚠ tasks/inbox 尚在 — events 仅 legacy 写"
        :note "详见 core :: legacy-zone 每表状态")

      (writer-removed credentials-writers
        :removed-finding "v0.4.15 调查发现 — credentials 表零 Rust CRUD"
        :was "无实际 writer (仅创建 schema)"
        :investigation "grep 全 crates/ 只找到 resolve_llm_credentials() 函数名字符串 (不操作此表)"
        :action "推荐 DROP — 见 legacy-zone 下 credentials"))

    (module-core
      (desc "观测 (3) + infra 游标 + backfill (7) + legacy-zone (4)")

      ;; ── 观测类 plumbing (3) ──
      (plumbing observability-incidents
        (desc "告警 / 异常聚合 — event-bus IncidentEvent 集中落库")
        :table "incidents"
        :schema-cols "id (PK) / severity / source / title / description / server_id / raw_payload / board_task_id / dedupe_key / created_at"
        :indexes "(created_at), (dedupe_key, created_at)"
        :scoping-candidate "加 project_id 可按项目告警分级"
        :trait-methods "ObservabilityStore: insert_incident / has_recent_incident / list_incidents"
        :code "crates/missiond-core/src/db/incident.rs + pg/observability.rs")

      (plumbing router-chat-archive
        (desc "路由聊天的压缩历史归档 side-table (主数据在 conversations with chat_type='router_chat')")
        :table "router_chat_archive"
        :schema-cols "id (PK auto) / original_id / session_id / role / content / timestamp / archived_at / archive_reason"
        :indexes "(session_id), (archived_at)"
        :relationship "与 conversations 表主数据配套 — archive 是压缩后的历史; archive_reason 记录触发场景"
        :code "pg/observability.rs (多处 INSERT INTO router_chat_archive)")

      (plumbing vision-image-cache
        (desc "图片描述缓存 — 独立无 FK, 按 image_hash 去重")
        :table "image_descriptions"
        :schema-cols "image_hash (PK) / media_type / description / char_count / created_at"
        :scoping "global (非 conversation-scoped; 同一图片跨 session 复用)"
        :migrated-from "v0.4.4 从 conversation-logs 迁入 (无 FK, 是全局图片 → 文本 cache)")

      ;; ── infra 游标 plumbing (4) ──
      (plumbing watcher-cursors
        (desc "文件监听器的字节偏移游标")
        :table "watcher_cursors"
        :schema-cols "file_path (PK) / byte_offset BIGINT / session_id? / updated_at"
        :purpose "文件 tail 式 watcher 的断点续读"
        :trait-methods "load_watcher_cursors / upsert_watcher_cursors_batch / delete_watcher_cursor")

      (plumbing reconcile-watermarks
        (desc "JSONL 对账 worker 的文件游标")
        :table "reconcile_watermarks"
        :schema-cols "jsonl_path (PK) / last_reconciled_size BIGINT / last_reconciled_at TIMESTAMPTZ"
        :migration "20260320000000_reconcile_watermarks.sql")

      (plumbing gemini-cli-watermarks
        (desc "Gemini CLI 会话文件的消息数水位")
        :table "gemini_cli_watermarks"
        :schema-cols "session_file (PK) / session_id / last_msg_count BIGINT / last_reconciled_at"
        :indexes "(session_id)"
        :migration "20260321000000_gemini_cli_watermarks.sql")

      (plumbing consumer-watermarks
        (desc "通用消费者水位 — 复合 PK 支持多 consumer × 多 session")
        :table "consumer_watermarks"
        :schema-cols "consumer_name + session_id (复合 PK) / last_processed_msg_id / last_processed_time / extra (JSON)"
        :indexes "(consumer_name)")

      ;; ── backfill plumbing (2, active via embedding-worker) ──
      (plumbing backfill-progress
        (desc "embedding-worker 的 phased 作业进度 — 每 phase 一行")
        :table "backfill_progress"
        :schema-cols "phase (PK) / status (default 'pending') / last_cursor BIGINT / total_estimated / processed / failed / started_at / completed_at / updated_at"
        :known-phases "conv_topic_vectors / conv_summary (session-level embedding)"
        :code "crates/missiond-core/src/db/backfill.rs (9 inherent 方法)")

      (plumbing backfill-failures
        (desc "embedding-worker 的 phase 内失败重试记录")
        :table "backfill_failures"
        :schema-cols "session_id + phase (复合 PK) / retry_count / last_error / updated_at")

      ;; ── daemon 级 KV plumbing (1) ──
      (plumbing daemon-state-kv
        (desc "daemon 级 key-value 状态")
        :table "daemon_state"
        :schema-cols "key (PK) / value TEXT / updated_at"
        :trait "SlotStore::daemon_state_get / _set"
        :note "trait 归 SlotStore (同文件 db/slot.rs), 实际用途是 daemon 全局 KV")

      ;; ── Legacy zone (4 表, 含调查后分级) ──
      (legacy-zone
        (desc "v0.4.15 实地调查后的 4 张老版 schema 分级")

        (table tasks
          :status "✓ KEEP — legacy slot API 仍在使用"
          :writers "crates/missiond-core/src/db/task.rs + db/pg/slot.rs"
          :schema-cols "id (PK) / role / prompt / status (default 'queued') / manual / slot_id / session_id / result / error / created_at / started_at / finished_at / notified_at"
          :indexes "4: (status), (role), (created_at), (manual)"
          :trait "SlotStore trait (insert_task / update_task / get_task / get_tasks_by_status / get_queued_tasks_by_role / requeue_running_tasks_for_slot / get_all_tasks / ack_completed_tasks)"
          :action "保留; 若未来 slot API 全换 board_tasks, 可再 drop")

        (table inbox
          :status "⚠ DEPRECATE — 轻度使用"
          :writers "crates/missiond-core/src/db/task.rs (insert_inbox_message / get_inbox_messages / mark_inbox_read)"
          :schema-cols "id (PK) / task_id / from_role / content / read / created_at"
          :indexes "(read), (created_at)"
          :trait "SlotStore trait (3 inbox 方法)"
          :mcp "mission_inbox (共用 misc handler)"
          :action "标 deprecated; 下轮清理时 drop")

        (table events
          :status "❌ DROP-CANDIDATE — dead (pillar 四 event_log 已取代)"
          :writers "仅 task.rs :: insert_event (legacy 路径, 无外部 active 消费)"
          :schema-cols "id (PK auto) / task_id / type / data / timestamp"
          :indexes "(task_id), (timestamp)"
          :trait "SlotStore trait (insert_event / get_events_by_task, 标 legacy)"
          :action "推荐下个 migration DROP; 需确认无外部系统查询"
          :superseded-by "pillar 四 event_log (SSOT)")

        (table credentials
          :status "❌ DROP-CANDIDATE — dead schema (v0.4.15 新发现)"
          :writers "无 Rust CRUD 代码"
          :readers "无"
          :schema-cols "id (PK) / knowledge_id (FK → knowledge ON DELETE CASCADE) / name / value_encrypted / created_at / updated_at"
          :investigation "grep 全 crates/ 显示 'credentials' 在代码中仅作函数名字符串 (resolve_llm_credentials 不操作此表); 表创建但零 CRUD"
          :action "推荐下个 migration DROP — 除非有未发现的运行时写入")))

    (module-egress
      (desc "MCP 读 + daemon 内部 consumer")

      ;; ── MCP readers (5) ──
      (reader mcp-incident
        :tool "mission_incident"
        :reads "incidents"
        :code "daemon/src/handlers/comm/question.rs (共用 handler, 与 mission_question / llm_trace 等)")

      (reader mcp-router-chat
        :tools "mission_router_chat / mission_router_chat_manage"
        :reads "router_chat_archive + conversations (chat_type='router_chat')"
        :code "daemon/src/handlers/comm/router_chat.rs")

      (reader mcp-sys-config-logs
        :tools "mission_sys_config / mission_sys_logs / mission_daemon_update"
        :reads "daemon_state + 各种 watermark 表 + backfill_progress"
        :code "daemon/src/handlers/system.rs")

      (reader mcp-infra-query
        :tools "mission_infra_query / mission_infra_ops"
        :reads "reconcile_watermarks / gemini_cli_watermarks / consumer_watermarks / watcher_cursors"
        :code "daemon/src/handlers/sysinfra/infra.rs")

      (reader mcp-inbox-legacy
        :tool "mission_inbox"
        :reads "inbox (legacy v1 API)"
        :code "daemon/src/handlers/sysinfra/misc.rs (共用 misc handler)")

      ;; ── daemon 内部 readers (4) ──
      (reader vision-enrichment
        :cross-ref "daemon 内消息 enrichment 流程"
        :reads "image_descriptions (按 image_hash 查)"
        :purpose "消息含图片时查找缓存的 LLM 描述")

      (reader backfill-resume
        :cross-ref "embedding_worker.rs 启动时恢复"
        :reads "backfill_progress (get_phase) + backfill_failures (retryable_failures)"
        :purpose "daemon 重启后从上次 phase 继续")

      (reader watermark-consumers-readers
        :cross-ref "多 consumer workers"
        :reads "consumer_watermarks / watcher_cursors / reconcile_watermarks / gemini_cli_watermarks"
        :purpose "每 worker 启动时读自己的游标做增量处理")

      (reader incident-enrichment
        :cross-ref "handlers/comm/timeline.rs (类似 gemini_requests enrichment 模式, 待确认)"
        :reads "incidents (可能 enrich 部分 Timeline 事件)"
        :status "🚧 待确认 — 类比 llm-support timeline-enrichment 模式"))

    (module-tables-owned
      (desc "本模块独占 14 张表 (10 active + 4 legacy)")
      (tables
        ;; active 观测 (3)
        (incidents              :status "✓ active")
        (router_chat_archive    :status "✓ active")
        (image_descriptions     :status "✓ active")
        ;; active infra (7)
        (watcher_cursors        :status "✓ active")
        (reconcile_watermarks   :status "✓ active")
        (gemini_cli_watermarks  :status "✓ active")
        (consumer_watermarks    :status "✓ active")
        (backfill_progress      :status "✓ active (embedding-worker 持续使用)")
        (backfill_failures      :status "✓ active")
        (daemon_state           :status "✓ active")
        ;; legacy (4)
        (tasks                  :status "⚠ keep — legacy slot API")
        (inbox                  :status "⚠ deprecate")
        (events                 :status "❌ drop-candidate (event_log 取代)")
        (credentials            :status "❌ drop-candidate (v0.4.15 新发现 dead schema)"))
      (count 14)
      (active-count 10)
      (legacy-count 4)

      (migration-history
        "v0.4.11: category 持 20 张表"
        "v0.4.1: system_timeline 合并到 pillar 四 event_log (SSOT)"
        "v0.4.13: gemini_requests / gemini_file_uploads / token_usage_ledger → module llm-support"
        "v0.4.14: slot_sessions / slot_tasks / dynamic_slots → module slot-support"
        "v0.4.15: category 升级为 module + legacy-zone 明确 4 张 drop 状态 + credentials 新发现 dead"))

    (cross-module-trait-sharing
      (desc "本 module 涉及的 trait 跨模块共享关系 — 同 llm-support / slot-support 的阻抗不匹配模式")
      :primary-traits
        ("ObservabilityStore (db/traits.rs:614) — 大 trait, 本模块用其中的: incident 方法 (3) + watcher_cursors (3) + reconcile_watermarks (3) + gemini_cli_watermarks (2) + consumer_watermarks via watermark_* (5) + router_chat (22)"
         "SlotStore (db/traits.rs:478) — 大 trait, 本模块用其中的: daemon_state (2) + legacy tasks (8) + inbox (3) + events (2)")
      :inherent-methods "backfill.rs 9 方法是 MissionDB inherent 方法, 不在 trait"
      :other-modules-use-same-traits
        ("llm-support: ObservabilityStore 的 gemini_log + gemini_file_cache + token_usage + token_stats 方法"
         "conversation-logs: ObservabilityStore 的 label_* + conversations_missing_summary_* 方法"
         "slot-support: SlotStore 的 slot_sessions + slot_tasks + dynamic_slots 方法")
      :design-note "trait 按实现文件切分 (slot.rs + observability.rs + gemini_log.rs + incident.rs 等) — 同 llm-support / slot-support 的阻抗不匹配模式, 接受"))


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
      :automation "sqlx::migrate! 编译期检查 + 运行期执行")

    (capability embedding-storage-governance
      (desc "embedding 列的 schema + policy SSOT — 存储端契约, 给 worker (生成) + search-engines (消费) 共同遵守")
      :role "schema + policy authority for embedding columns (NOT generator, NOT search)"
      :dimension 512
      :provider-binding "qwen3 via pillar 二 2.2 sonnet-gateway (唯一 provider, 禁止降级兜底)"
      :index-type "HNSW (pgvector extension, cosine similarity)"

      (storage-tables
        (desc "5 张表有 embedding_vec 列 — 按模块归属")
        (knowledge
          :column "embedding_vec"
          :type "vector (standard)"
          :hnsw-params "m=16, ef_construction=64"
          :owned-by "module kb-manager")
        (conversation_topic_vectors
          :column "embedding_vec"
          :type "vector"
          :hnsw-params "m=16, ef_construction=64"
          :owned-by "module conversation-logs")
        (message_embeddings
          :column "embedding_vec"
          :type "halfvec (1KB 压缩)"
          :hnsw-params "m=24, ef_construction=128"
          :rationale "消息量大, halfvec 节省空间 + 更深索引"
          :owned-by "module conversation-logs")
        (skill_topics
          :column "embedding_vec"
          :type "vector"
          :hnsw-params "m=16, ef_construction=64"
          :owned-by "module project-management")
        (ast_nodes
          :column "embedding_vec"
          :type "vector"
          :hnsw-params "m=16, ef_construction=64"
          :owned-by "module kb-manager"))

      (skip-audit-policy
        :table "message_embedding_skips"
        :role "audit trail — 记录跳过 embed 的消息 ID + skip_reason"
        :schema "message_id (PK, FK→conversation_messages.id) + skip_reason TEXT"
        :owner-module "conversation-logs"
        :writer "embedding_worker.rs :: insert_message_embedding_skips_batch (backfill 分桶时批量写)"
        :decision-source "embedding_worker.rs :: should_embed() 返 EmbedDecision enum"
        :readers "db/pg/conversation.rs 的统计查询 (COUNT + GROUP BY skip_reason)"
        :note "⚠ 重要: 这是 audit log 不是 dedup 表. 实际去重在 EmbedDecision enum pattern 做, skip 表只是留痕"
        :reasons-examples "tool_result / system message / empty / role-based skip")

      (retention
        :strategy "follow-parent"
        :reason "embedding 是 parent row 的派生物, 无独立 TTL"
        :note "parent row 删除 → embedding 随之失效 (CASCADE 或应用层清理)")

      (contract-with-generator
        :generator "pillar 二 2.3 workers/sonnet/embedding_worker.rs"
        :invariant-1 "失败 fail-fast 不兜底 (见 embedding-worker :provider)"
        :invariant-2 "批量写入, 单次事务 (insert_message_embeddings_batch / set_conversation_topic_vectors / kb_remember)"
        :invariant-3 "provider 固定 qwen3, 换需重算全表")

      (contract-with-consumer
        :consumer "pillar 二 2.6 search-engines :: engine vector-hnsw"
        :invariant-1 "consumer 必须容忍 NULL embedding (async 生成未就绪或跳过)"
        :invariant-2 "查询时用同 provider 生成 query 向量, 否则相似度不可比"
        :fusion-with "FTS + trigram + tag (见 pillar 二 2.6 fusion-ranker)")

      (change-protocol
        :dim-change "改 512 → 1024: 需同时迁移 5 承载表 + 5 HNSW 索引 + provider 切换"
        :provider-change "换 qwen3 → 别的: 需重算所有 5 表 embedding; 中间态不可搜, 建议离线 backfill"
        :new-table-adding-embedding "(1) 迁移加 embedding_vec 列 + HNSW 索引 (2) 本 capability 的 storage-tables 补记 (3) embedding-worker 加写入路径")

      (known-issue-v0.4.6-moved
        "skill_topics embedding 归 project-management 而非 kb-manager: 因 skills 是 project-management 管; 但 search-engines 查 skill 时仍通过 vector-hnsw 统一路径"
        "ast_nodes embedding 归 kb-manager (kb-ast-linkage + code-search 消费)")))


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
      "    memory pillar ownership: 5+4+9+14+20 = 52 张 (从 v0.4.3 的 56 下降 4)"
      "v0.4.5 (2026-04-19 — workflow 合并):"
      "(T) pillar 五 action-instruction-specs 的 workflow-lisp-templates + flow-yaml-templates 合并"
      "    → 单一 component workflows 带 (kind methodology) + (kind executable)"
      "    methodology = .missiond/workflows/*.lisp (human, 抽象叙事, 非执行)"
      "    executable  = $MISSIOND_HOME/flows/*.yaml (machine, 具体节点, flow-engine-v2 执行)"
      "    受众/粒度/执行性分轴, 但概念统一为'多步工作流规约'"
      "v0.4.6 (2026-04-19 — embedding 治理):"
      "(U) cross-cutting 新增 capability embedding-storage-governance (schema + policy SSOT)"
      "    确定 5 承载表: knowledge / conversation_topic_vectors / message_embeddings / skill_topics / ast_nodes"
      "    发现 message_embeddings 用 halfvec (1KB 压缩), HNSW 参数 m=24 ef=128 (vs 其他 4 表 m=16 ef=64)"
      "    澄清 conversations.summary_embedding 列不存在 (之前 lisp 错列, 已修)"
      "(V) 5 处散落描述改用 cross-ref 指向 governance SSOT (kb/conv writer / non-db-forms / pillar 二 2.6)"
      "    减少维护负担 + 单点真理"
      "v0.4.7 (2026-04-19 — board 按'memory=库'原则精简):"
      "(W) pillar 二 2.4 orchestration 新增 2 engine component: autopilot + flow-engine-v2"
      "    autopilot: tick pipeline / dispatch-logic / writes-to-memory / 5-10s tick / CAS 决策"
      "    flow-engine-v2: 5 node types / execution-model / flow_context persist"
      "(X) board 模块精简: 把 autopilot + flow-engine-v2 + autopilot-prompt-snapshot + autopilot-tick-scan"
      "    改为 cross-ref 到 pillar 二 engines, 保留'库侧视角' (schema + interface + FSM 声明)"
      "(Y) board module-ingress 新增 :principle 'memory=库' 原则声明"
      "(Z) plumbing state-machine 的 :recovery 改为 :recovery-interface + :recovery-scheduling 双字段"
      "    接口在库, 时机由 engine 决定"
      "v0.4.8 (2026-04-19 — agent 协作共享内存层正式化):"
      "(AA) board 新增 helper agent-execution-coordination — 从 intent-event-bus-execution.lisp 提取模式"
      "    6 shared-memory-slots: phase-tracker / claims / deviations / decisions / completions / issues"
      "    storage: Lisp 文件约定 .missiond/v2/<name>-execution.lisp 配套 frozen 设计 lisp"
      "    manager-interface: TBD (future MCP: mission_execution_claim/decide/deviate/complete)"
      "    linked-concepts: methodology (pillar 五 workflows, '怎么做' reusable) vs execution (本 helper, '这次做的过程' operational)"
      "    design-rationale: board = 任务编排中心, 并行 agent = 多 task 协作"
      "(BB) pilot-instance intent-event-bus-execution.lisp 成为首个正式案例 (D001-D016 / DC001-DC049 / I001-I010)"
      "v0.4.9 (2026-04-19 — conversation-logs 按'memory=库'原则精简):"
      "(CC) 10 个 worker writer 全改 cross-ref 到 pillar 二 2.3 workers/<kind>/<worker_name>"
      "     每条加 :library-pov 标明'库只暴露 store 接口 + 类型约束; worker 算法在 pillar 二'"
      "(DD) 新增 (writer-removed worker-briefing) 墓志铭 — 反映 SSOT cutover (v1.3.0) 实际删除的 briefing_worker.rs"
      "     helper narration-briefing → narration: :generator 改为 step-narrator, 加 :history 注记"
      "     goal-3 solution 移除 briefing, 加 :note 说明语义摘要能力已失"
      "(EE) 2 egress 消费者改 cross-ref: context-pipeline-history / harvester-for-kb"
      "     库不再假装它们是自己的 reader; 它们是 daemon compute 或跨模块 worker"
      "(FF) pillar 二 2.3 workers 同步修正: sonnet 组 6→5 (briefing 删) + 增 :writes-to-memory 注记"
      "     每组标明各 worker 写入哪个 memory 模块, 补强 lisp↔code 同构"
      "v0.4.10 (2026-04-19 — kb-manager 按'memory=库'原则精简):"
      "(GG) ingress 6 writer → 1 MCP + 5 cross-ref + 1 context-pipeline cross-ref"
      "     5 worker (arch-maintenance / experience-harvester / tagger-chunker / ast-sync / embedding) 全 cross-ref"
      "     context-pipeline-kb-audit 明确标为 daemon 内部非 worker"
      "(HH) core plumbing code-indexing 的 :maintained-by → :maintenance-cross-ref"
      "     core path kb-code-awareness 的 :maintained-by → :library-pov 强调库只暴露表接口"
      "(II) core helper access-audit + operation-queue 加 :library-pov 分清'接口在库' vs '触发在 compute'"
      "     operation-queue 的 :consumer 改 :consumer-cross-ref 标 ⚠ TBD 已知缺口"
      "(JJ) egress 6 reader → 4 MCP 库 + 2 compute 消费者 cross-ref"
      "     context-pipeline-retrieval / worker-code-prefetch 改 cross-ref + :library-pov"
      "v0.4.11 (2026-04-19 — project-management 按'memory=库'原则精简):"
      "(KK) ingress 5 writer 重组:"
      "     - 2 MCP 库 writer 保留 (mcp-project-mutation / mcp-skill-mutation)"
      "     - lisp-survey-worker-snapshot → cross-ref pillar 二 2.3 workers/sonnet"
      "     - project-memory-vault-edit → vault-md-edit 加 :library-pov (用户 / Claude Code 直接编辑文件)"
      "     - project-scope-propagation 移出 ingress (它不是 writer, 是跨模块约定)"
      "(LL) plumbing scope-mechanism 整合 (write-propagation-convention) 子块"
      "     列 4 个 propagators 说明 conversations/knowledge/board_tasks/retrospective 的 project_id 如何填"
      "     标明是 'library-side invariant', 每个 worker 实现在各自模块"
      "(MM) 4 模块 'memory=库' 精简全部完成: board / conversation-logs / kb-manager / project-management")

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
