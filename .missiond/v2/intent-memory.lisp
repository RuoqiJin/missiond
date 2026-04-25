;; ══════════════════════════════════════════════════════
;; MissionD — Memory Pillar (Full Spec)
;; Parent:  v2/intent.lisp :: pillar memory
;; Status:  草稿 v0.4 — 4 成熟模块各管自己的表 + 精简 system-support
;; Created: 2026-04-19
;; ══════════════════════════════════════════════════════
;;
;; v0.4.x 重构演进 — 8 个 module 自治 + 5 surface 正交维度 (v0.4.18 pillar-interfaces):
;;
;;   ┌── 5 business module (各自 in/core/out + 显式 owned-tables) ──┐
;;   │  module project-management     5 张 (projects + 4 skills)      │
;;   │  module board                  4 张 (board_tasks 系列)         │
;;   │  module kb-manager             9 张 (knowledge + ast 索引)     │
;;   │  module conversation-logs     15 张 (claude/gemini/codex 会话) │
;;   │  module directive-layer        3 张 (directive/plan/workflow)  │
;;   └──────────────────────────────────────────────────────────────┘
;;   ┌── 4 support module ──────────────────────────────────────────┐
;;   │  module llm-support            3 张 (gemini/token 观测)        │
;;   │  module slot-support           3 张 (slot 运行时)              │
;;   │  module system-support        14 张 (infra 游标 / legacy)      │
;;   │  module embedding-support      0 张 (5 承载表 + 1 audit 的列契约)│
;;   └──────────────────────────────────────────────────────────────┘
;;   ┌── 横切 ────────────────────────────────────────────────────┐
;;   │  db-trait / retention / migrations                             │
;;   └──────────────────────────────────────────────────────────────┘
;;
;; ══════════════════════════════════════════════════════

(intent memory
  (version "v0.5.8"
    :polish "v0.5.0 L1 语义压缩 (2026-04-20): history + migration-log 外移到 intent-memory-history.lisp; table-catalog 表详情 SSOT 合并到各 module (保持 count + 表名索引). 不做 L2/L3 DSL 压缩 (会破坏 :binds-to / :library-pov 的 grep 明显性 + 施工 agent 体验). 行数: 2999 → 2629 (-12.7%)."
    :v0.5.1-patch "v0.5.1 (2026-04-21 — gptpro agent-execution-coordination 协议升级): (EEEE) 回应 D010 重复编号 — execution-log helper 从 pilot 升格为正式协议. id-counters / claims-with-lease / audit/repair / derived-indexes 等结构性升级. 12 MCP actions (mission_execution *) 设计完整. 未来 execution lisp 按协议产生, 避免手工编号复发 D010 类问题."
    :v0.5.2-patch "v0.5.2 (2026-04-25 — recursive-contract alignment): agent-execution-coordination helper 补 ingress/core/egress 与实现状态标注; xjp-router embedding provider 归属校正为 worker pillar. 不改表归属,只补架构标注."
    :v0.5.3-patch "v0.5.3 (2026-04-25 — file-first implementation alignment): directive-layer 补 intent-alignment.lisp / PLAN.lisp / workflow.lisp 三段文件产物契约,允许先文件评审再镜像 DB,服务后续 MissionD 工位代码同构闭环."
    :v0.5.4-patch "v0.5.4 (2026-04-25 — capability usage monitor): system-support 新增 capability-usage-read-model; 当前代码用 conversation_tool_calls + board_tasks.flow_template + MCP/YAML registry 形成 tool+flow 调用热度快照,只产出治理候选,不自动删除; event_log/workflow stats/semantic merge 留后续增强."
    :v0.5.5-patch "v0.5.5 (2026-04-25 — directive/plan/workflow manager alignment): directive-layer 标注 mission_directive / mission_plan / mission_workflow 管理面已代码同构; compile/distill 仍为 dry-run, LLM compiler/distiller actors 待实现."
    :v0.5.5-pipeline-note "v0.5.5 (2026-04-25 — unified-entry-pipeline file-first 加固): directive-layer file-first-artifacts 补 status lifecycle / review-gate-owner / evidence sidecar 契约, 与 flow pillar F-intent-alignment-plan-execution-loop + intent-layer section unified-entry-pipeline 对齐. 不改表归属与 schema; 仅补文件级 lifecycle 与 DB 镜像约定."
    :v0.5.6-patch "v0.5.6 (2026-04-25 — actor v0 status backfill + semantic evidence v1): directive-layer 标注 directive-compiler v0 (compiler_mode=sonnet) / plan-compiler v0 (compiler_mode=sonnet) / plan-runner v0 + auto-selection v1 (sexp hint parsing, 单节点 dispatch) / workflow-distiller v0 (distill_mode=sonnet) / methodology compiler v0 (compile_mode=deterministic + run_methodology) / generated flow loader 已 code-aligned partial; capability-usage-read-model 升级到 semantic evidence v1 (5 sources + lisp hint merge-candidate); 不改 schema/表归属, 仅回填 implementation status. file-first .lisp writer/sync / 高阶 semantic lifting / forge compiler / planner-class model alias / 完整 PLAN DAG scheduler / ExecutionEvent dispatch metadata 仍 code-alignment pending."
    :v0.5.7-patch "v0.5.7 (2026-04-26 — wave 11 plan-dag-scheduler 设计): file-first-artifacts 新增 artifact plan-node-state-projection (architecture-designed) — 完整 PLAN DAG scheduler 启用后 evidence sidecar 增 nodes[<node_id>] block (per-node attempts/claim_id/start-end/inner_result/acceptance_pass/rollback_path); 不改 directive/plan/workflow 表 schema, sidecar 仍是 file-first; v0 plan_runner_dispatch entry shape 必须向后兼容; writer plan-runner-v0 标注 v1-future-target=writer plan-dag-scheduler (复用 agent-execution-coordination claim/lease 协议, D010 教训不允许自建 ID 池). 完整 11-stage 协议 + 节点 schema 见 intent-layer pillar :: section action-instruction-actor :: actor plan-dag-scheduler 与 flow pillar :: F-intent-alignment-plan-execution-loop :: s6 :: dag-scheduler."
    :v0.5.8-patch "v0.5.8 (2026-04-26 — execution handoff protocol): agent-execution-coordination 升级为双平面协议 — execution companion Lisp 是 control plane (claim/heartbeat/deviation/decision/issue/completion/verification), scoped git commit 是 durability plane (防止并行 agent stash/pop/Edit 回退成果). completions 扩展 changed_files/staged_files/verification/commit_hash/commit_status; 新增 git-handoff slot 与 scoped-commit-contract. 当前为 architecture-designed + task-file operational-practice; mission_execution complete schema 与 daemon enforce 仍 code-alignment pending.")
  (parent "v2/intent.lisp :: pillar memory")
  (created "2026-04-19")
  (history
    :see "intent-memory-history.lisp :: memory-history :: short-version-log"
    :contains "v0.1 → v0.5.0 完整演进日志 (每版本 1 句总结)"
    :note "长历史外移, 减少主 lisp context 消耗 (v0.5.0 L1 polish)")
  (status "8 module + 5 surface 定稿 (v0.4.18 pillar-interfaces + v0.4.19 rename + v0.4.20 cleanup); 可演进")

  (purpose "系统长期记忆 — 5 business module + 4 support module (共 9 个) + 5 surface (pillar-interfaces v0.4.18) + 横切")
  (storage "PostgreSQL via sqlx::PgPool")
  (gateway "crates/missiond-core/src/db/ — 唯一 DB 入口")

  ;; ─────────────────────────────────────────────────────────
  ;; Path Convention — v0.5.0 加入 (施工后 line 号 stale, 改用函数/概念标签)
  ;; ─────────────────────────────────────────────────────────
  (path-convention
    (desc "lisp 中 file path 约定, 帮 agent 精准 seek 代码")
    :prefix-aliases
      ("db/*     → crates/missiond-core/src/db/*")
      ("pg/*     → crates/missiond-core/src/db/pg/*")
      ("types/*  → crates/missiond-core/src/types/*")
      ("daemon/src/* → crates/missiond-daemon/src/*")
      ("mcp/src/*    → crates/missiond-mcp/src/*")
      ("migrations/* → crates/missiond-core/migrations/*")
    :reference-format
      "file + :: function/struct/trait/concept (稳定, 不随施工变化)"
      "避免 file:line_number 形式 (行号施工后 stale)"
    :examples
      ("traits.rs :: ConversationStore trait — agent grep 'trait ConversationStore' 即可定位")
      ("autopilot.rs :: tick-time intent lookup — agent grep 'get_recent_intents' 定位调用点")
      ("engine/intent_engine/memory_scheduler.rs :: dispatch_queued_submit_tasks")
    :stale-line-numbers-policy "v0.5.0 起不再用 line 号. 已有的 file:line 引用视为 'as-of-施工前 snapshot, may drift'")

  (migrated-out
    "embedding-provider → worker pillar :: xjp-router-gateway (qwen3 独立 provider, code-aligned for embedding)"
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
      :owned-by "module system-support (+ module slot-support for slot_*)"
      :examples "daemon_state / credentials / inbox / events / tasks (system-support); slot_sessions/tasks/dynamic_slots (slot-support); watcher_cursors / consumer_watermarks / reconcile_watermarks / backfill_* (system-support)"
      :note "v0.4.1: system_timeline 已移除 — 合并进 pillar 四 event_log (见 migration-log)")

    (group candidates-for-promotion
      (rationale "⚠ 目前全局无 project_id, 按项目分可能有价值")
      (candidates
        (retrospective_results :benefit "项目级复盘归档"     :owner-note "module conversation-logs; 可经 session_id → conversations.project_id 推断")
        (token_usage_ledger    :benefit "项目级成本追踪 — 最想看的指标" :owner-note "module llm-support")
        (prompt_snapshots      :benefit "项目级 prompt 调优"  :owner-note "module board; PK=task_id 可经 board_tasks.project_id 推断")
        (incidents             :benefit "项目级告警分级"     :owner-note "module system-support")
        (gemini_requests       :benefit "项目级 LLM 使用模式" :owner-note "module llm-support")
        (agent_questions       :benefit "项目级 agent 问题集" :owner-note "module board; 可经 task_id → board_tasks.project_id 推断")
        (slot_tasks            :benefit "项目级 slot 任务"   :owner-note "module slot-support (v0.4.14 已归)")
        (user_intents          :benefit "项目级意图识别"     :owner-note "module conversation-logs (v0.4.16 校正: 实际从未真正迁出, ConversationStore trait 仍拥有 6 个方法); 缺 project_id 列")
        (intent plan workflow  :benefit "specs 三表应加 project_id" :owner-note "module directive-layer (v0.5.5): store+manager code-aligned partial, compiler/distiller actors 待 pillar 五 启用")
        (ast_nodes ast_file_meta beacons beacon_nodes :benefit "项目级代码索引" :owner-note "module kb-manager; 缺列")
        (skill_topics skill_blocks skill_versions skill_executions :benefit "项目私有技能库" :owner-note "module project-management; 缺列")
        (image_descriptions    :benefit "项目级图片注释"    :owner-note "module system-support; 独立按 hash 去重")
        (router_chat_archive   :benefit "项目级 router 归档" :owner-note "module system-support")
        (flows-yaml-files      :benefit "项目私有 flow 模板" :kind "文件, 非 DB"))))


  ;; ═════════════════════════════════════════════════════════════
  ;;  Pillar Interfaces — memory pillar 对外 4 个接口维度
  ;;  v0.4.18 引入: 按消费者类别切 surface, 和 8 module 的业务切分正交
  ;;  module 是 SSOT (具体 writer/reader 定义); 本 section 是契约层 (语义/稳定性/协议)
  ;;  每个 writer/reader 通过 :binds-to [:surface-name] 绑定, 形成 module × surface 反向索引
  ;; ═════════════════════════════════════════════════════════════
  (pillar-interfaces
    (desc "memory pillar 对外暴露的 4 个接口维度 — 按消费者类别切, 和 8 module 的业务切分正交")
    (design-principle "module 是 SSOT (具体 writer/reader 定义), pillar-interfaces 是契约层 (语义 + 稳定性 + 协议)")
    (binding-protocol "每个 module 的 writer/reader 通过 :binds-to [:surface-name] 指向本层, 形成 module × surface 反向索引")
    (created "v0.4.18 (2026-04-20)")

    ;; ─────────────────────────────────────────────
    ;; Surface 1: MCP Surface — 对 MCP client 暴露
    ;; ─────────────────────────────────────────────
    (interface mcp-surface
      (purpose "对 MCP client(Claude Code / 其他 Agent)暴露的 CRUD + query API")
      (protocol "JSON-RPC over stdio (rmcp)")
      (protocol-impl-by "pillar 三 (工具)"
        :scope "rmcp server / dispatcher / stdio transport / schema validation 框架"
        :boundary "memory 管 tool 内容层 (name / 参数 schema / 返回值 / 行为 / 错误码); 工具 pillar 管协议层 (JSON-RPC framing / dispatch / framework-level validation)"
        :施工-note "本 pillar 代码同构只涉及 tool 内容 (tools/*.rs 里的 tool 定义 + handlers/*.rs 里的 handler 实现); 底层 rmcp framework 代码归工具 pillar, 同构不动")
      (code-location "crates/missiond-mcp/src/tools/*.rs + crates/missiond-daemon/src/handlers/*.rs")
      (stability-contract
        (breaking "参数 schema / response schema 破坏性变更 → 走 forge 冲压 major version")
        (additive "加新 tool / 加可选参数 → 走 forge minor version")
        (lifecycle "tool 弃用需 deprecated 标记 + 至少 1 版本过渡期"))
      (current-tool-families
        (family-project "mission_project / mission_intent / mission_skill_*")
        (family-board "mission_board_create / _query / _update / _claim / _retry / _delete / _decompose / _note_add")
        (family-kb "mission_kb_query / _mutate / _remember / _ops / mission_insight / mission_code_search / mission_universe_graph")
        (family-conversation "mission_conversation_query / _analyze / _reconcile / mission_retrospective_manage / mission_audit")
        (family-observability "mission_llm_trace / mission_cost_report (🚧) / mission_incident / mission_infra_query")
        (family-slot "mission_slots / mission_slot_history / mission_compute_slot")
        (family-directive-layer "mission_directive / mission_plan / mission_workflow (code-aligned partial: read/control + draft persistence; compiler actors pending)")
        (family-system "mission_sys_config / mission_sys_logs / mission_inbox / mission_router_chat")))

    ;; ─────────────────────────────────────────────
    ;; Surface 2: Worker-Trait Surface — 对 daemon 内部 Rust 代码暴露
    ;; ─────────────────────────────────────────────
    (interface worker-trait-surface
      (purpose "对 pillar 二 workers / engines / daemon 内部所有 Rust 代码暴露的 trait 接口")
      (protocol "async trait in missiond-core::db::traits + PG impl in db/pg/*.rs")
      (code-location "crates/missiond-core/src/db/traits.rs + db/pg/*.rs")
      (stability-contract
        (breaking "trait 方法签名变更 → 触发 cascade lint + 所有 impl 同步改")
        (additive "加新方法 → default impl 或 require all impls")
        (vs-mcp "trait 可先于 MCP 工具实现 (库早于 API), 这是 v0.4.10+ cross-ref 分工的基础"))
      (cross-trait-sharing-policy "trait 按 .rs 文件切, module 按业务切, 允许 trait 跨 module (impedance mismatch 显式披露)")

      (trait-organization-principle
        :added-in "v0.4.24 (施工 Phase 2C 大量实战后归纳)"
        :desc "9 primary traits vs sub-traits 的设计原则 — 给施工 agent 明晰边界"
        :primary-traits "lisp 声明的 9 个, 是 memory 对外稳定契约 (MCP handlers + workers 依赖的)"
        :sub-traits "代码层可能按 .rs 文件切出的细粒度 trait (如 ToolCallStore / EventStore 等) — 实现细节, 不在 lisp 列表"
        :merging-rule "sub-trait 随时可合并入 primary trait (按业务 module 归属). Phase 2C 合并了 5 个: SkillStore→ProjectStore / ToolCallStore+EventStore+RetrospectiveStore→ConversationStore / VisionStore→ObservabilityStore"
        :rust-coherence-constraint "⚠ Rust 一 type 一 trait 只能一个 impl block. 合并后方法全部集中到一个 .rs 文件 (如 pg/skill.rs 承载 ProjectStore 全 33 方法, pg/project.rs 仅 helper)"
        :cross-boundary-judgment
          "施工时遇到跨 trait 归属模糊的方法 (如 conversations_missing_summary_cursor 算 conversation 分析还是 infra watermarks), 判断原则:"
          "1. 业务语义优先 — 这方法解决什么业务问题"
          "2. 数据源二 — 读/写哪张表 → 归表所在 module"
          "3. 不确定时保留在原 trait, 加 :boundary-note 说明"
      )
      (current-traits
        (ProjectStore      :module "project-management" :scope "projects + skill_* 4 张")
        (BoardStore        :module "board"              :scope "board_tasks + notes + questions + prompt_snapshots")
        (KbStore           :module "kb-manager"         :scope "knowledge + edges + access_log + operation_queue + ast + beacon")
        (ConversationStore :module "conversation-logs"  :scope "conversations + messages + turns + events + user_intents + labels, 36+ 方法, traits.rs :: ConversationStore trait body" :cross-module "user_intents 的 6 方法")
        (MessageStore      :module "conversation-logs"  :scope "messages 细粒度 + 搜索 + 分页, traits.rs :: MessageStore trait body")
        (ObservabilityStore :spans ["llm-support" "system-support" "conversation-logs"]
                            :scope "gemini_requests + file_uploads + token_ledger + incidents + labels, 36 方法, traits.rs :: ObservabilityStore trait body"
                            :note "典型 cross-module trait, 按 .rs 文件切粒度")
        (DirectiveLayerStore  :module "directive-layer"       :scope "directive + plan + workflow, 20 方法, code-aligned partial; directive_list_recent / plan_list_recent / workflow_get_by_id added for manager surfaces")
        (SlotStore         :spans ["slot-support" "conversation-logs"]
                           :scope "slot_sessions + slot_tasks + dynamic_slots, traits.rs :: SlotStore trait body")
        (InfraStore        :module "system-support"     :scope "watermarks (13) + backfill (9) + daemon_state (2) = 24 方法"
                           :methods-signature-v0.4.24
                             "watermarks: watermark_get / watermark_advance_time / watermark_advance_msg_id / watermark_advance_full / watermark_list (5, consumer_watermarks)"
                             "watermarks: load_watcher_cursors / upsert_watcher_cursors_batch / delete_watcher_cursor (3, watcher_cursors)"
                             "watermarks: get_reconcile_watermark / upsert_reconcile_watermark / get_all_reconcile_watermarks (3, reconcile_watermarks)"
                             "watermarks: load_gemini_cursors / save_gemini_cursor (2, gemini_cli_watermarks)"
                             "backfill: backfill_get_phase / backfill_list_phases / backfill_start_phase / backfill_update_progress / backfill_complete_phase / backfill_record_failure / backfill_retryable_failures / backfill_retryable_failures_no_cooldown / backfill_clear_failure (9)"
                             "daemon_state: daemon_state_get / daemon_state_set (2)")
        (TimelineStore     :owned-by "pillar 四 event-bus (非 memory)"
                           :why-listed-here "MissionStore super-trait bound 包含它, memory pillar 代码同构中不动; 实际是 pillar 四 projection 读接口, delegate 到 event_log"
                           :code-location "db/pg/timeline.rs (pillar 四 所有)"
                           :callers "ws/server.rs catch-up + handlers/comm/timeline.rs 的 5 MCP 工具")))

    ;; ─────────────────────────────────────────────
    ;; Surface 3: Frontend Surface — 对前端 WS 暴露
    ;; ─────────────────────────────────────────────
    (interface frontend-surface
      (purpose "对前端 WS 暴露的实时数据订阅流")
      (protocol "WS frames on event_log subscribers via daemon bus bridge")
      (code-location "crates/missiond-daemon/src/bus/ws_bridge.rs")
      (stability-contract
        (breaking "WS event kind 删除 / payload schema 变更 → 前端同步更新")
        (additive "加新 event kind → event_log 加 kind + frontend 订阅扩展")
        (note "WS bridge 本质也是 worker-trait-surface 的一种 (Rust 读 trait 发 WS), 但契约形态是 WS event 不是 trait 方法, 单独列"))
      (current-streams
        (stream-conversation "新 messages / turns / events — from conversation-logs")
        (stream-board        "board_tasks 状态变更 / notes / questions — from board")
        (stream-project      "projects CRUD / slot 状态 — from project-management + slot-support")
        (stream-timeline     "event_log 订阅 → 前端 timeline view (来自 pillar 四 event-bus)")
        (stream-llm-trace    "gemini_requests 实时 (🚧 潜在, 未实现)")))

    ;; ─────────────────────────────────────────────
    ;; Surface 4: Cross-Pillar Surface — 对其他 pillar 暴露
    ;; ─────────────────────────────────────────────
    (interface cross-pillar-surface
      (purpose "对其他 pillar 暴露的事件 + schema 宿主关系 — lisp 层契约, 非单一代码点")
      (protocol "多种: event_log kind 约定 / schema 承载 / config 读取 / trait 调用")
      (stability-contract "跨 pillar 契约改动需对应 pillar 同步修改 + 事件总线 SSOT 遵守 v1.3.3 architecture-unlocked direct-edit-with-record")

      (to-pillar-二-workers
        (mechanism "workers 通过 worker-trait-surface 的 trait 方法 CRUD 本 pillar 的表")
        (cross-ref "见 worker-trait-surface :: current-traits")
        (note "每个 module 的 ingress/egress 的 worker cross-ref 全部归此维度"))

      (to-pillar-四-event-bus
        (mechanism "本 pillar 的 modules 作为 event producer, 写 event_log 表")
        (cross-ref "pillar 四 event-bus (v1.3.3 architecture-unlocked) 的 event_log 接收来自 memory 9 module 的业务事件")
        (producer-modules-list "conversation-logs (message-ingested) / board (task-state-changed) / kb-manager (knowledge-added) / 等"))

      (to-pillar-五-intent-layer
        (mechanism "pillar 五 actor (pending) 将是 memory directive-layer module 的 intelligent writer; MCP manager 已提供 dry-run/draft 管理入口")
        (schema-hosting "intent / plan / workflow 3 张表 schema 在 memory (v0.4.17), actor 在 pillar 五")
        (cross-ref "见 module directive-layer :: pending-implementation-checklist"))

      (to-pillar-六-system
        (mechanism "memory modules 读 pillar 六 system_config 获取 DB 连接串 / retention 策略")
        (note "每个 module 启动时读 config, module 内不重复声明 — pillar 六 是全局配置源"))

      (to-pillar-七-flow
        (mechanism "flow-engine-v2 通过 worker-trait-surface 读写 board_tasks + prompt_snapshots")
        (note "flow-engine 本质是 pillar 二 worker 的一种, 统一走 worker-trait-surface"))

      (memory-hook-pipeline
        :added-in "v0.4.24 (施工发现) → v0.5.0 (已迁到 board_tasks)"
        :desc "memory pillar '记忆自动触发 LLM' 业务机制 — v0.5.0 从独立 tasks 表迁移到 board_tasks 主队列"
        :pipeline-v0.5
          "1. conversation 落地 / KB 更新 → 触发 memory hook"
          "2. state::submit_task (memory pillar 入口) → INSERT board_tasks (trigger_source='memory_hook', role='memory')"
          "3. memory_scheduler (pillar 二 engine) 轮询 board_tasks WHERE trigger_source='memory_hook' AND status='open'"
          "4. dispatch_queued_submit_tasks → BoardStore::claim_board_task → 派给某 slot"
          "5. slot 执行 prompt → BoardStore::update_board_task (status='done' + result)"
          "6. recover_stale_running_tasks (BoardStore 现成接口) 清理超时"
        :active-callers-in-memory-pillar
          "state.rs::submit_task (入口, 写 board_tasks)"
          "handlers/knowledge/kb.rs (记忆钩子触发点)"
          "workers/local/conversation_logger.rs (会话落地触发)"
          "workers/local/pty_event_worker.rs (PTY 事件触发)"
          "handlers/knowledge/board.rs (已经直接用 board_tasks, 无需改)"
        :pillar-二-consumers
          "engine/intent_engine/memory_scheduler.rs (v0.5 改为 poll board_tasks)"
        :v0.5-migration-history
          "v0.4.x: memory-hook 用独立 tasks 表 (legacy slot API)"
          "v0.5.0: 迁到 board_tasks (统一任务 SSOT), 加 trigger_source 列区分来源. tasks 表 drop, SlotStore 8 tasks 方法删"))

    ;; ─────────────────────────────────────────────
    ;; Surface 5: External-Filesystem Surface — 非 DB 文件系统直写/直读
    ;; ─────────────────────────────────────────────
    (interface external-filesystem
      (purpose "memory pillar 管辖的 non-db-forms — 文件系统级读写, 不经 DB interface")
      (protocol "OS-level file IO (Claude Code Edit tool / 用户手编 / worker 文件写)")
      (code-location "无集中入口 — 各 non-db-form 各自通过文件系统操作")
      (stability-contract
        (breaking "文件路径约定 / 文件格式 (lisp / md / yaml / jsonl) 变更 → 所有直接 IO 方都要同步")
        (additive "加新文件变体 → 在 non-db-forms section 声明")
        (note "因为不经 trait, 契约靠文件约定 + 目录结构传达; 易出锅需小心管理"))
      (current-endpoints
        (per-project-intent-file ".missiond/intent.lisp — 代码快照, worker 写 / mcp_intent 读 / Claude Code 直读")
        (project-claudemd         "<project>/CLAUDE.md — 项目级 Claude 指令")
        (auto-memory-vault        "~/.claude/projects/{encoded}/memory/*.md — 跨会话记忆, 用户/Claude Code 编")
        (pty-jsonl-source         "~/.claude/projects/{encoded}/*.jsonl — 外部产, conversation-logger 消费"))
      (contrast-with-mcp "MCP 工具里的 mission_intent(action=read) 是 MCP surface 暴露, 但底层读的是本 surface 的文件 — 表层 surface 上是 MCP, 数据源走文件; :binds-to 标最贴近的那层")))


  ;; ═════════════════════════════════════════════════════════════
  ;;  Target Code Layout — memory pillar 代码同构目标布局
  ;;  v0.4.22 补充 — 施工前必读, 明确同构范围 + 边界 + 目录决定
  ;; ═════════════════════════════════════════════════════════════
  (target-code-layout
    (desc "memory pillar 代码同构目标布局 — 施工第一步参照")
    :principle "按 9 module + 5 surface 正交组织; SSOT 按业务 module 切; trait 按 .rs 文件切 (接受阻抗不匹配, 显式披露)"
    :created "v0.4.22 (2026-04-20)"
    :purpose "对施工 agent 明确: 什么算本 pillar 的代码 / 什么是 out-of-scope / 代码目录组织原则"

    (in-scope
      (desc "本 lisp 管辖代码 — 同构重构范围")
      (core-crate       "crates/missiond-core/src/db/"
        :purpose "schema + trait + PG impl (worker-trait-surface 实现)"
        :files "按 .rs 文件切 trait (project.rs / board.rs / knowledge.rs / conversation.rs / slot.rs / observability.rs / gemini_log.rs / incident.rs / ast.rs / question.rs / task.rs / backfill.rs)")
      (core-types       "crates/missiond-core/src/types/"
        :purpose "Rust struct + enum (FSM / EmbedDecision / 各表 row 类型)")
      (migrations       "crates/missiond-core/migrations/"
        :purpose "PG schema 演进 SSOT"
        :authority "migrations 里的 CREATE TABLE 是 schema 唯一真相")
      (mcp-tools        "crates/missiond-mcp/src/tools/"
        :purpose "MCP tool 定义 (mcp-surface 的 client-facing 形态)")
      (daemon-handlers  "crates/missiond-daemon/src/handlers/"
        :purpose "MCP tool handler 实现 — 调 trait, 做业务编排"
        :note "handler 物理上在 daemon crate, 逻辑上实现 memory 的 MCP 契约, 属 memory 管辖")
      (daemon-ws-bridge "crates/missiond-daemon/src/bus/ws_bridge.rs"
        :purpose "memory → 前端 WS (frontend-surface)"
        :note "ws_bridge 整个文件横切 (也有 event-bus frames), memory 只管和本 pillar stream 相关部分"))

    (out-of-scope
      (desc "本 lisp :cross-ref 指向但不施工的代码 — 由对应 pillar 负责")
      (pillar-二-workers  "crates/missiond-daemon/src/workers/"
        :reason "writer/reader 的 :cross-ref 指向这里, 但 worker 的 tick / 算法 / recovery 由 pillar 二 管")
      (pillar-二-engines  "crates/missiond-daemon/src/engine/"
        :reason "autopilot / flow-engine / learning engine 归 pillar 二")
      (pillar-四-event-bus "crates/missiond-daemon/src/event/ + bus/ (非 ws_bridge 部分)"
        :reason "event_log 等 4 表 + 事件总线 v1.3.1 frozen")
      (pillar-五-actor    "TBD"
        :reason "directive-layer 未来 writer 在 pillar 五, 本 pillar 只声明 schema+trait 契约")
      (施工规则 "同构只动 in-scope; 对 out-of-scope 的 :cross-ref 只做 '指向的文件/函数存在性' 验证, 不改内容, 不做重构"))

    (layout-decision-directory-structure
      :question "要不要把代码按 module 重组成 crates/missiond-core/src/memory/<module>/ 这种目录?"
      :decision "❌ NO — 保持 flat trait-file 切分"
      :reason-1 "trait 跨 module 现实普遍: ObservabilityStore 跨 3 module (llm-support / system-support / conversation-logs), SlotStore 跨 2 module (slot-support / system-support + conversation-logs); 目录化制造假整洁"
      :reason-2 "按 trait 文件切 + 现有代码阻抗最小; 重组成本 >> 收益"
      :reason-3 "lisp 的 module 概念已足够 cohesive; 代码层通过 pillar-interfaces :: worker-trait-surface :: current-traits 做 module ↔ trait ↔ files 三方映射即可"
      :concrete-result "每个 memory module 对应若干 trait (:primary-traits) + 可能共享的 trait (:consumes-shared-trait); 每个 trait 对应一个 .rs 文件 (db/*.rs + db/pg/*.rs)"
      :v0.4.24-rust-coherence-addendum
        "⚠ Rust coherence: 一 type (PgMissionStore) 对一 trait (e.g. ProjectStore) 只能一个 impl block. 故 sub-trait 合并 primary 后, 所有方法集中到一个 .rs 文件:"
        "- pg/skill.rs 承载 ProjectStore 全 33 方法 (合并 SkillStore 后, pg/project.rs 瘦身为 helper)"
        "- pg/conversation.rs 承载 ConversationStore 全 ~114 方法 (合并 ToolCallStore + EventStore + RetrospectiveStore)"
        "施工 agent 合并 sub-trait 时遵守: 选 1 个 .rs 承载完整 impl, 另一个文件转 helper 或删")

    (drop-execution-principle
      :added-in "v0.4.24 (施工 Phase 6 经验)"
      :desc "执行 drop migration 时的安全守则 — 从 lisp legacy-zone 标签到代码"
      :rules
        "1. 只 drop lisp 明标 'DROP-CANDIDATE' 或 'DROPPED' 的表. 标 KEEP/DEPRECATE 的不碰"
        "2. drop 前 grep 全 repo 确认 caller 活跃度 (caller-count). 标 KEEP 但 caller-count 30+ 的属 🔴 CORE, 绝不盲目 drop"
        "3. trait 方法同步删 — 否则 impl 引用不存在的表, 编译失败"
        "4. 对应 caller 改写或删 (如 Phase 6 删 step_narrator 整文件 + 改 5 处 handler)"
        "5. migration 文件用 DROP TABLE IF EXISTS 幂等"
        "6. 保留墓志铭 — lisp 的 legacy-zone 条目改 status ✅ DROPPED, 留 pre-drop-schema + removed-at, 供后续知情"
      :mcp-tool-judgment
        "MCP tool 的删除:"
        "- 若 tool 对应表被 drop 且 lisp 未说明保留: 删 handler + dispatcher 路由 (如 mission_session_narrations)"
        "- 若 tool 对应表保留 (如 inbox): 保留 tool 原样, 不动"
        "- 若 tool 有未来计划: 降 deprecated stub + 注释说明")

    (file-to-module-mapping
      (desc "代码文件 → memory module 的映射 — v0.4.24 Phase 2 施工后终极落地")
      :status "✅ v0.4.24 更新为施工完成后实况 (原 v0.4.22 骨架已过期, Phase 2 后结构大变)"
      :mapping-style "按 Rust coherence: 一 trait 一 impl block per type. 故 impl 文件位置 ≠ module 归属"

      (db-root-layer
        :desc "db/ 根下 8 文件 (trait 定义 + shared types + 中间层)"
        (traits.rs            :serves "all 9 modules"
                              :purpose "MissionStore super-trait 聚合 + 9 primary trait + TimelineStore (pillar 四)"
                              :traits "ProjectStore / BoardStore / KbStore / ConversationStore / MessageStore / ObservabilityStore / SlotStore / InfraStore / DirectiveLayerStore + TimelineStore (pillar 四)")
        (shared.rs            :serves "all" :purpose "Row struct + enum 通用类型")
        (error.rs             :serves "all" :purpose "DbError + DbResult")
        (mod.rs               :serves "all" :purpose "db 模块定义 + 工具 fn")
        (project.rs           :serves "project-management" :purpose "helper + re-export (D001 补建)")
        (observability.rs     :serves "cross-module" :purpose "helper + re-export (D001 补建)")
        (directive.rs         :serves "directive-layer" :purpose "row mapping (Stage 2B.2 新建)")
        (conversation_query.rs :serves "conversation-logs" :purpose "FTS query builder helper"))

      (db-pg-layer
        :desc "db/pg/ 13 文件 (PG impl, 按 Rust coherence 切)"
        (mod.rs               :serves "all" :purpose "PG 模块 + PgMissionStore struct")
        (board.rs             :serves "board"              :impls "BoardStore (41 方法)")
        (knowledge.rs         :serves "kb-manager"         :impls "KbStore (56 方法, 含 ast/beacon)")
        (message.rs           :serves "conversation-logs"  :impls "MessageStore (15 方法)")
        (conversation.rs      :serves "conversation-logs"  :impls "ConversationStore ~114 方法 (合并 ToolCall+Event+Retrospective 后)")
        (project.rs           :serves "project-management" :purpose "helper 模块 (实际 impl 在 skill.rs)")
        (skill.rs             :serves "project-management" :impls "ProjectStore 全 33 方法 (Rust coherence: 承载 SkillStore 合并后的统一 impl block)"
                              :coherence-note "因 Rust 一 trait 一 impl block, pg/project.rs 的 8 方法也挪到此处")
        (observability.rs     :serves "llm-support + system-support + conversation-logs"
                              :impls "ObservabilityStore 58 方法 (2D 后 -22 拆 InfraStore + 合并 VisionStore)")
        (slot.rs              :serves "slot-support + conversation-logs (sessions)"
                              :impls "SlotStore 38 方法 (2D 后 -2 daemon_state 拆 InfraStore)")
        (infra.rs             :serves "system-support"     :impls "InfraStore 24 方法 (2D 填充: watermarks 13 + backfill 9 + daemon_state 2)")
        (directive.rs         :serves "directive-layer"    :impls "DirectiveLayerStore 17 方法 (Stage 2B.2 新建: directive 6 + plan 6 + workflow 5)")
        (timeline.rs          :owned-by "pillar 四"         :impls "TimelineStore (projection delegate), memory 不管")
        (migrate_from_sqlite.rs :status "DEPRECATED" :gated "all(sqlite, postgres)" :purpose "一次性迁移工具, 当前 feature set 永不编译"))

      (types-layer
        :desc "types/ 下 Row struct + FSM enum"
        ("types/directive.rs" :serves "directive-layer" :content "Directive/Plan/Workflow struct + DirectiveStatus/PlanStatus enum")
        ("types/其他"          :serves "按 module" :note "未全列, 细节不是施工热点"))

      (mcp-layer
        :desc "MCP 工具 — lisp 归 memory, 物理分布"
        (mcp-tools   :location "crates/missiond-mcp/src/tools/"           :scope "tool 定义 (client-facing)")
        (mcp-handlers :location "crates/missiond-daemon/src/handlers/"    :scope "tool handler 实现 (调 trait)"))

      (ws-bridge
        :file "crates/missiond-daemon/src/bus/ws_bridge.rs"
        :serves "frontend-surface" :scope "5 streams 路由")

      (eliminated-wild-files-v0.4.24
        :desc "Phase 2 施工过程中消除的文件, 记录供知情"
        (gen_*.rs×16          "Stage 2A.1 删 (两套 forge 旧产物均未被 mod.rs 导入)")
        (KnowledgeStore       "Stage 2A.2 rename → KbStore")
        (db/timeline.rs       "Stage 2A.3 删 (551B 冗余空壳, pillar 四 归属)")
        (SkillStore trait     "Stage 2C.1 合并进 ProjectStore")
        (ToolCallStore / EventStore / RetrospectiveStore "Stage 2C.2-4 合并进 ConversationStore")
        (VisionStore trait    "Stage 2C.5 合并进 ObservabilityStore")
        (sqlite/ 整目录 + db/ sqlite-gated 21 文件 "Stage 2E -14018 LOC")
        (step_narrator.rs     "Phase 6 删 (worker, 属 pillar 二)")
        (narration trait 9 方法 + events trait 2 方法 "Phase 6 zombie cleanup")))

    (施工-roadmap
      (phase-1 "file-to-module-mapping 扫描补齐 + 校验"
        :action "施工 agent 扫 in-scope 目录, 对每个 .rs 文件生成 '属于哪个 module / 实现哪个 trait / 方法列表' 清单, 补完本 section")
      (phase-2 "按 module 生成 impl-checklist"
        :action "每个 module 生成一张 MD checklist: trait 声明 vs 实际 impl + cross-ref 文件指向 vs 实际文件存在"
        :output-location "施工 execution lisp 的 shared-memory 槽位")
      (phase-3 "填 DirectiveLayerStore (全新 trait)"
        :action "按 module directive-layer pending-implementation-checklist 6 步执行"
        :dependency "phase-1+2 完成")
      (phase-4 "binds-to 验证"
        :action "每个 writer/reader :cross-ref 指向的代码函数在 in-scope 范围内的确存在; out-of-scope 的只做文件存在性 ping")
      (phase-5 "agent-team 验证 lisp ↔ code 同构"
        :action "派独立 agent 双向校验: (a) lisp 每个 module 的 trait/writer/reader 在代码里找得到 (b) 代码的 struct/trait/impl 在 lisp 里也被声明")
      (phase-6 "(可选) 野生清理 v2 + pending-drop 表实际 drop migration"
        :action "v0.4.22 标的 2 张 narrations + 4 legacy (tasks/inbox/events/credentials drop-candidate) 执行 migration")))


  ;; ═════════════════════════════════════════════════════════════
  ;;  Future Support Extensions — memory 对未来扩展的开放姿态
  ;;  v0.4.23 补充 — 告诉施工 agent / 未来 pillar 设计者: memory 怎么接纳新需求
  ;; ═════════════════════════════════════════════════════════════
  (future-support-extensions
    (desc "memory pillar 对其他 pillar 持久化需求的开放姿态 — '数据先于 module' 原则")
    :principle "memory 的 support module (llm-support / slot-support / system-support / embedding-support) 都是 '数据先存在 → 后归类成 module' 建立的, 不预占位. 未来其他 pillar 若需持久化, 按同路径来."

    (threshold-for-new-support-module
      :criterion-1 "≥3 张表 (单张表归现有 module 如 system-support 即可)"
      :criterion-2 "≥1 个稳定 trait (按 .rs 文件自然切出)"
      :criterion-3 "≥1 个稳定 writer (worker / MCP handler / actor 实际在写)"
      :decision "三条都满足 → 开新 module; 否则放现有 module (特别是 system-support 作为杂项收容所)")

    (known-candidates
      (desc "已知可能会要 memory support module 的外部 pillar — 触发建 module 时按此名开始")

      (tools-support
        :for "pillar 三 (工具)"
        :probable-tables "tool_invocations (调用审计) / tool_permissions (权限) / tool_rate_limits (限流) / tool_schemas (参数 schema 版本)"
        :trigger "工具 pillar 设计确认至少 3 张表落 PG 后, 开 module tools-support"
        :cross-ref "类比 module llm-support 管 LLM 调用观测; tools-support 管 tool 调用观测 + 治理"
        :status "🚧 未触发 (工具 pillar 尚未设计)")

      (directive-actor-support
        :for "pillar 五 (意识层 / intent-layer actor 未来实现后)"
        :note "当前 directive-layer module 管 directive/plan/workflow 3 张 schema; actor 启用后若需额外表 (比如 actor_state / compiler_metrics) 再按此路径新增"
        :status "🚧 未触发 (actor 未实现)"))

    (non-candidates
      (desc "这些 pillar 不需要 memory support module — 它们的持久化已由 pillar 自己管或不需要")
      (pillar-二-workers "worker 状态在 slot_sessions/dynamic_slots (已归 slot-support); watermark 在 system-support — 无新 support")
      (pillar-四-event-bus "event_log 等 4 表自己管, 不归 memory")
      (pillar-六-system "system_config 在 system-support (daemon_state); 不需新 support")
      (pillar-七-flow "flow-engine-v2 读写 board_tasks.flow_context, 落 module board; 不需新 support"))

    (触发流程
      1 "外部 pillar 设计阶段发现持久化需求 → 清点表 + trait + writer"
      2 "满足 threshold 3 条件 → 在外部 pillar 的 lisp 里声明 'requires memory :: module <name>-support'"
      3 "memory 同步开新 module (走 llm-support / slot-support 等的标准模式)"
      4 "双方 cross-ref 指向彼此, 施工 agent 按本 pillar 的 target-code-layout 做同构")

    (anti-pattern
      "❌ 预先建空 module 占位 — 违反 '数据先于 module' 原则, 制造虚幻结构"
      "❌ 小规模数据 (1-2 表) 开新 module — 应放 system-support 杂项收容"
      "❌ 跨 pillar 写入脱离 memory — 持久化必须经 memory trait, 不允许其他 pillar 直连 PG"))


  ;; ═════════════════════════════════════════════════════════════
  ;;  Table Catalog — 61 张 PG 表按模块 ownership 归类
  ;; ═════════════════════════════════════════════════════════════
  (table-catalog
    (desc "60 PG 表索引. L1 polish: 表详情 SSOT 在各 module 的 (module-tables-owned ...); 本 section 只列名单 + count.")
    (total 60)
    (source "crates/missiond-core/migrations/ — CREATE TABLE 实际统计")
    (ssot-ref "各 module :: module-tables-owned 为 :purpose / :scoping / :schema 真相")

    (by-module
      (module-project-management  5 [projects skill_topics skill_blocks skill_versions skill_executions])
      (module-directive-layer     3 [directive plan workflow])
      (module-board               4 [board_tasks board_task_notes agent_questions prompt_snapshots])
      (module-kb-manager          9 [knowledge knowledge_edges kb_access_log kb_operation_queue kb_ast_links ast_nodes ast_file_meta beacons beacon_nodes])
      (module-conversation-logs  15 [conversations conversation_messages conversation_turns conversation_events conversation_tool_calls conversation_topic_vectors conversation_labels message_embeddings message_embedding_skips message_narrations narration_cursors message_translations message_labels retrospective_results user_intents])
      (module-llm-support         3 [gemini_requests gemini_file_uploads token_usage_ledger])
      (module-slot-support        3 [slot_sessions slot_tasks dynamic_slots])
      (module-system-support     12 [incidents router_chat_archive image_descriptions watcher_cursors reconcile_watermarks gemini_cli_watermarks consumer_watermarks backfill_progress backfill_failures daemon_state inbox tasks])
      (pillar-four-event-bus      4 [event_log event_subscriptions blob_storage dlq]
                                     :owned-by "pillar 四, 非 memory"))

    (status-annotations
      (tasks        :status "⚠ PARTIAL MIGRATION v0.5.0 — memory-hook 迁 board_tasks, 表保留给 pillar 二 compute")
      (inbox        :status "⚠ DEPRECATE — caller 1 (pillar 二 strategy_worker)")
      (events       :status "✅ DROPPED v0.4.24 Phase 6")
      (credentials  :status "✅ DROPPED v0.4.24 Phase 6")
      (narrations-2 :status "✅ DROPPED v0.4.24 Phase 6 (message_narrations + narration_cursors)")
      :note "v0.5.0 后 system-support 从 14 → 12 (events + credentials drop)"))


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
        :debounce "60s per project_id"
        :managed-by "module project-management (v0.4.20: 作为 lisp-survey trigger 源, 归项目管理)"))

    (form embedding-vectors
      (desc "pgvector 二进制 embedding 列 — 归 module embedding-support 治理 (v0.4.21)")
      :managed-by "module embedding-support"
      :governance-ssot "memory :: module embedding-support :: module-core :: plumbing dimension-contract / storage-tables-catalog (v0.4.21 从 cross-cutting capability 升格)"
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
        :owned-by "module llm-support")))


  ;; ═════════════════════════════════════════════════════════════
  ;;  模块 1: Project Management
  ;;  项目作为记忆的作用域单位; 管 projects + specs + skills
  ;; ═════════════════════════════════════════════════════════════
  (module project-management
    (desc "项目管理: 注册 + 作用域 + 技能(skills); 规约(directive/plan/workflow)已迁 directive-layer module")
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
        :binds-to [:mcp-surface]
        :tools  "mission_project init / sync / set_active / vault_sync / import_universe / survey"
        :writes "projects"
        :code   "daemon/src/handlers/knowledge/project.rs")

      (writer mcp-skill-mutation
        :binds-to [:mcp-surface]
        :tools  "mission_skill_mutate"
        :writes "skill_topics / skill_blocks / skill_versions / skill_executions"
        :code   "daemon/src/handlers/knowledge/skill.rs")

      ;; ── 外部 worker 驱动 (cross-ref) ──
      (writer lisp-survey-worker-snapshot
        :binds-to [:worker-trait-surface]
        :binds-to-note "写文件不走 trait, 但 projects.intent_path 的 UPDATE 走 trait; 归 worker-trait-surface"
        :cross-ref "pillar 二 2.3 :: workers/sonnet/lisp_survey_worker.rs"
        :writes    "<project>/.missiond/intent.lisp (per-project 代码快照 FILE, 非 DB)"
        :library-pov "库不管 FILE 的存在, 只在 projects.intent_path 存路径指针; 扫描 + 写文件策略在 worker"
        :not-writes "DB directive/plan/workflow 表 — 这些在 memory directive-layer module (v0.4.17, v0.4.19 rename)")

      ;; ── 外部 actor (用户 / Claude Code 直接编辑文件) ──
      (writer vault-md-edit
        :binds-to [:external-filesystem]
        :binds-to-note "不经 memory pillar 的 DB interface, 也不经 MCP; 纯文件系统级"
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
        :binds-to [:mcp-surface]
        :tools "mission_project get / list / context / memories / sync / vault_sync"
        :reads "projects + 多表聚合 + vault")

      (reader mcp-intent-view
        :binds-to [:mcp-surface]
        :tool "mission_intent read/section/summary/list"
        :reads "<project>/.missiond/intent.lisp 文件 (only — 描述项目代码真实状态)"
        :not-reads "directive DB 表 (那是 memory directive-layer module 管, v0.4.17+v0.4.19)")

      (reader mcp-skill-view
        :binds-to [:mcp-surface]
        :tools "mission_skill_query / mission_skill_context"
        :reads "skill_topics / blocks / versions / executions")

      (reader frontend-project-stream
        :binds-to [:frontend-surface]
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
      (removed-in-v0.4.4 "intent / plan / workflow 迁到 pillar 五 intent-layer (action/instruction 层)")
      (v0.4.16-correction "user_intents 之前声称迁 pillar 五 是错的; 实际 writer=intent_analyst/reader=autopilot, trait=ConversationStore, 归 conversation-logs")
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
        :binds-to [:mcp-surface]
        :tools "mission_board_create / update / claim / decompose / retry / note_add / delete"
        :count 7
        :writes "board_tasks / board_task_notes"
        :code "daemon/src/handlers/knowledge/board.rs")

      (writer mcp-question-lifecycle
        :binds-to [:mcp-surface]
        :tool "mission_question (独立 MCP 工具, 非 board 工具面)"
        :writes "agent_questions (+ 自动 UPDATE 关联 board_tasks.status='blocked')"
        :code "daemon/src/handlers/.../question.rs")

      ;; ── 外部 engine 驱动的写入 (计算逻辑不在库内) ──
      (writer autopilot
        :binds-to [:worker-trait-surface]
        :cross-ref "pillar 二 2.4 orchestration :: component autopilot"
        :writes "board_tasks (CAS claim / status 推进 / lease 回收) + prompt_snapshots (task 执行存档)"
        :library-pov "库只暴露 BoardStore 原子操作 + prompt_snapshots schema; tick / 决策 / recovery 时机 由 engine 管")

      (writer flow-engine-v2
        :binds-to [:worker-trait-surface]
        :cross-ref "pillar 二 2.4 orchestration :: component flow-engine-v2"
        :writes "board_tasks.flow_context (每节点 persist)"
        :library-pov "库只暴露 update_board_task 接口 + flow_context JSONB 列; 节点执行顺序 / 变量插值 由 engine 管")

      ;; v0.5.0 新增: memory-hook 迁入 board_tasks
      (writer memory-hook-submitter
        :added-in "v0.5.0 (从 legacy tasks 表迁入)"
        :binds-to [:worker-trait-surface]
        :cross-ref "memory pillar 多 caller: state.rs::submit_task / handlers/knowledge/kb.rs / workers/local/conversation_logger.rs / workers/local/pty_event_worker.rs"
        :writes "board_tasks (role='memory' + trigger_source='memory_hook')"
        :library-pov "库暴露 insert_board_task + 新 list_board_tasks_by_trigger 查询接口; 触发时机 (会话落地/KB 更新/PTY 事件) 在 memory pillar 各 caller"
        :pipeline-ref "cross-pillar-surface :: memory-hook-pipeline (v0.5.0 版)"))

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
        :columns 28
        :v0.5-addition "trigger_source TEXT NULL (v0.5.0 新加, 记 memory_hook / user / autopilot / flow / cli; 替代 legacy tasks 表)"
        :grouping "身份 / 内容 / 生命周期 / 占用 / 层级 / 执行 / 流程 / 作用域 / 去重 / 触发源 / UI"
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

      ;; ── 并行 agent 协作的共享内存层 — v0.5.1 gptpro 升级为正式协议 ──
      (helper agent-execution-coordination
        (desc "并行 agent 协作的共享内存层 — 从单次 pilot 升级为可复用的正式协议")
        :status "v0.5.8 recursive-contract; protocol code-aligned via mission_execution manager + ExecutionEvent live projection; scoped commit durability plane architecture-designed"
        :implementation-status
          (:file-protocol "implemented-by-practice — intent-event-bus-execution.lisp / intent-memory-execution.lisp / worker-pillar-execution.lisp 已实战复用"
           :schema-shape "architecture-designed — meta/id-counters/phase-tracker/claims/deviations/decisions/issues/completions/git-handoff/derived-indexes"
           :manager-code "code-aligned — crates/missiond-daemon/src/handlers/knowledge/agent_execution.rs implements 12 actions"
           :mcp-tool "code-aligned — mission_execution exposed via crates/missiond-mcp/src/tools/knowledge/agent_execution.rs"
           :scoped-commit "architecture-designed + task-file operational-practice — mission_execution complete schema extension / scoped git guardrails pending")
        :actual-evidence
          ["intent-event-bus-execution.lisp — 首个成功 pilot"
           "intent-memory-execution.lisp — 第二次复用"
           "intent-memory-execution.lisp 出现重复 D010 — 暴露手工编号缺口"]

        (helper-recursive-contract
          :shape "helper = ingress(execution request / companion log) → logic-core(atomic coordination slots) → egress(status/audit/repair + optional board/event signals)"
          :unit "execution 是一次多 agent 协作任务实例; slot 是可验证的 operational state 原子"
          :rule-1 "memory owns durable protocol/file shape, not runtime manager mechanics"
          :rule-2 "ID allocation / lease / audit / repair must be manager-controlled before MCP write access is enabled"
          :rule-3 "execution-log records operational truth; methodology/workflow remains intent-layer owned"
          :rule-4 "status/audit can rebuild derived-indexes from durable slots; derived-indexes are cache, not truth"
          :rule-5 "execution companion Lisp is control plane; git commit is durability plane; neither replaces the other"
          :rule-6 "parallel code-writing agents must claim scope before editing and scoped-commit after verification")

        (helper-ingress
          (entry-1 "mission_execution(action=open|list|claim|heartbeat|release|deviate|decide|issue|complete|status|audit|repair)")
          (entry-2 "human/agent file edit following file-protocol (current implemented practice)")
          (entry-3 "board_task / flow_context linkage when execution scope is tied to a board task")
          (entry-4 "architecture-lisp checker before any manager commit")
          (entry-5 "git handoff request from workstation after verification (changed_files + staged_files + commit_hash)"))

        (helper-core
          :contract "把并行 agent 的协作状态写成固定 slot, 让 claim / ID / deviation / completion 不再靠口头约定"
          (step s1 "open/load execution: locate companion log, initialize/read meta + id-counters + phase-tracker")
          (step s2 "claim lifecycle: detect scope overlap, allocate claim_id, set lease_expires_at, require heartbeat, release or mark stale")
          (step s3 "record facts: deviate/decide/issue/complete allocate D/DC/I/COMP ids atomically and append typed entries")
          (step s4 "validate write: paren balance + schema shape + monotonic id + no open conflicting claim")
          (step s5 "rebuild derived-indexes: active_claims / open_issues / unresolved_deviations / latest_decisions / completed_phases")
          (step s6 "audit/repair: report semantic risks; apply only structural repair with explicit dry_run/apply mode")
          (step s7 "durability handoff: after verification, stage only claimed files, commit scoped patch, record commit_hash or commit_blocker"))

        (helper-egress
          :writes ["execution companion Lisp slots" "derived-indexes cache"]
          :reads ["parent design Lisp" "existing execution companion log"]
          :returns "status report / audit report / repair plan or receipt / scoped commit receipt"
          :optional-emits ["event-bus ExecutionEvent::*" "BoardTaskStatusChanged when linked to board scope"]
          :cross-ref-worker "intent-worker.lisp :: orchestration-governance :: agent-execution-manager-interface"
          :cross-ref-flow "intent-flow.lisp :: F-execution-log-governance"
          :cross-ref-tools "mission_execution MCP surface")

        (actual-findings-from-pilots
          (finding-1
            :fact "execution-log 已不只是 event-bus 特例, memory pillar 施工也在使用"
            :design-impact "helper 应从 'board 的一个小技巧' 升级为 board 下的正式 operational protocol")

          (finding-2
            :fact "仅靠 '写前 claim 下一个 ID' 的口头约定不够; memory execution log 已出现重复 D010"
            :design-impact "必须把 ID 分配从人工协商改成 manager 原子分配")

          (finding-3
            :fact "当前 claim 只有占用语义, 没有 lease / heartbeat / stale-claim 回收"
            :design-impact "多 agent 长时施工必须有 claim 生命周期")

          (finding-4
            :fact "phase / completion / evidence / deviation / issue 已经成为施工刚需"
            :design-impact "execution file 不应只是自由写日志, 应有固定 slot + 衍生索引")

          (finding-5
            :fact "frozen design lisp 与 execution lisp 的 pairing 已成为事实模式"
            :design-impact "file-governance 应正式要求 :companion-log, 并允许 status/audit/repair 工具消费"))

        ;; ── 正式存储协议 ─────────────────────────────────────
        (shared-memory-slots
          (meta
            :purpose "execution 实例元数据"
            :fields "execution_id / parent_design / status / opened_at / last_updated_at / owner / scope / companion_of")

          (id-counters
            :purpose "原子 ID 分配状态"
            :fields "next-claim-id / next-deviation-id / next-decision-id / next-issue-id / next-completion-id"
            :rationale "消除 D010 这种手工重复编号问题")

          (phase-tracker
            :purpose "phase 当前状态 + cursor + owner + started/completed"
            :fields "current_phase / phases[] / stage_cursor / checkpoints[]")

          (claims
            :purpose "谁锁了什么 scope"
            :fields "claim_id / claimer / scope / phase / acquired_at / lease_expires_at / heartbeat_at / released_at / status"
            :conflict-key "scope overlap"
            :reap-rule "lease_expires_at < now 且无 heartbeat → stale-claim")

          (deviations
            :purpose "frozen design 与实际实施偏差"
            :fields "deviation_id / lisp_said / actually_found_or_did / reason / approved_by / decided_action / status / at")

          (decisions
            :purpose "执行期小决策"
            :fields "decision_id / context / options / chosen / rationale / decided_by / at")

          (completions
            :purpose "阶段完成证据"
            :fields "completion_id / phase / agent / summary / deliverables / verification / changed_files / staged_files / commit_hash / commit_status / at"
            :commit-status-values "[not-required pending committed blocked skipped]"
            :rationale "让 status/audit 能发现 '报告完成但 live tree 没有成果' 的断层")

          (git-handoff
            :purpose "把 execution control-plane 与 git durability-plane 连接起来"
            :fields "handoff_id / claim_id / agent / claimed_scope / changed_files / staged_files / diff_check / verification_commands / commit_hash / commit_message / commit_status / blocker / at"
            :truth-rule "commit_hash 指向 durable code artifact; execution log 指向 operational context; 二者互为索引, 不互相替代"
            :scope-rule "staged_files 必须是 claimed_scope 子集; 否则 audit 报 scoped-commit-violation"
            :recovery-rule "commit_status=blocked 时必须有 blocker + changed_files, 后续 agent 可按 claim scope 接手")

          (issues
            :purpose "阻塞 / 风险 / 未决事项"
            :fields "issue_id / severity / desc / resolution_path / resolved_at / owner / at")

          (derived-indexes
            :purpose "供 status / audit 快速读取的衍生视图"
            :fields "active_claims / open_issues / unresolved_deviations / latest_decisions / completed_phases"
            :materialization "可从正文重建; 可选写回缓存"))

        ;; ── 对外接口 ────────────────────────────────────────
        (manager-interface
          :status "code-aligned; 4ab7994 upgrades 'Edit lisp 文件' practice into controlled manager"
          :implementation-plan "crates/missiond-daemon/src/handlers/knowledge/agent_execution.rs"

          (mcp-tool-design mission_execution
            :surface-status "code-aligned"
            :action-count 12
            (action open
              :args "execution_id + parent_design + scope + owner?"
              :writes "meta + id-counters 初始化 + phase-tracker 初始态")

            (action list
              :args "parent_design? + status? + scope_prefix? + limit?"
              :reads "execution companion logs / derived-indexes"
              :returns "execution summaries for dashboard / manager selection")

            (action claim
              :args "execution_id + phase + claimer_name + scope + lease_secs"
              :atomicity "先查 overlap claim; 无冲突后分配 claim_id 并写入"
              :writes "claims + active_claims index")

            (action heartbeat
              :args "execution_id + claim_id + claimer_name"
              :writes "claims.heartbeat_at + lease_expires_at 延长"
              :purpose "长任务保活")

            (action release
              :args "execution_id + claim_id + claimer_name + summary?"
              :writes "claims.status=released + released_at")

            (action deviate
              :args "execution_id + lisp_said + actually_found + reason + approved_by?"
              :id-allocation "manager 自动分配 D<NNN>"
              :writes "deviations")

            (action decide
              :args "execution_id + context + options + chosen + rationale + decided_by"
              :id-allocation "manager 自动分配 DC<NNN>"
              :writes "decisions")

            (action issue
              :args "execution_id + severity + desc + resolution_path? + owner?"
              :id-allocation "manager 自动分配 I<NNN>"
              :writes "issues")

            (action complete
              :args "execution_id + phase + agent_name + summary + deliverables? + verification? + changed_files? + staged_files? + commit_hash? + commit_status?"
              :id-allocation "manager 自动分配 COMP<NNN>"
              :writes "completions + phase-tracker"
              :side-effect "若 phase 全部完成可更新 meta.status"
              :durability-plane "architecture-designed — commit_hash 记录 scoped git commit; code-alignment pending")

            (action status
              :args "execution_id"
              :returns "meta + phase-tracker + active_claims + unresolved_deviations + open_issues + latest_decisions")

            (action audit
              :args "execution_id"
              :checks ["paren balance"
                       "ID 单调且无重复"
                       "claim overlap"
                       "stale claim"
                       "completion 是否覆盖每个 completed phase"
                       "open issue 是否有 owner"]
              :returns "audit report")

            (action repair
              :args "execution_id + mode(dry_run|apply)"
              :scope "仅修结构性问题: duplicate-id renumber suggestion / stale-claim mark / derived-index rebuild"
              :guard "不会 silently 改正文语义条目")))

        ;; ── 文件协议 ────────────────────────────────────────
        (file-protocol
          :current-format ".missiond/v2/<design-name>-execution.lisp"
          :required-pairing "每个 frozen / managed design lisp 都有 :companion-log"
          :write-mode "整体 read → validate → modify → write; 或 file lock"
          :rebuild-rule "derived-indexes 可删可重建, 正文 slots 才是 durable truth")

        ;; ── Git 交付协议 ───────────────────────────────────
        (scoped-commit-contract
          :status "architecture-designed; task-file operational-practice; daemon enforcement pending"
          :why "共享 execution Lisp 可证明 agent 做过什么, 但无法防止后续 stash/pop/Edit 回退 live tree; scoped commit 把成果持久化为 git object"
          :control-plane "mission_execution companion log — claim/heartbeat/deviation/decision/issue/completion/verification"
          :durability-plane "git commit — 只包含本 claim scope 内文件, 可回滚/复核/合并"
          :logic-core
            ((s1 claim "agent 开工前 mission_execution(action=claim) 锁定文件/模块 scope")
             (s2 work "只修改 claimed_scope; 如发现必须越界, 先 issue/deviate 并等待新 claim")
             (s3 verify "运行任务书验收命令 + git diff --check + architecture-lisp checker(若改 Lisp)")
             (s4 complete "mission_execution(action=complete) 写 changed_files/verification/remaining_risk")
             (s5 stage "git add 仅添加 claimed_scope 内文件; 禁止 git add .")
             (s6 preflight "git diff --cached --name-only 必须是 claimed_scope 子集")
             (s7 commit "git commit scoped patch; commit message 包含 execution_id/phase 或任务名")
             (s8 backfill "把 commit_hash 回填 completion/git-handoff; 若 commit blocked, 写 blocker 与 patch state"))
          :commit-policy
            ((code-alignment-new-session :commit "required" :reason "代码成果必须防止后续并行 agent 回退")
             (parallel-code-agent        :commit "required" :reason "并行工位共享 worktree 风险最高")
             (resident-lisp-architect    :commit "wave-boundary-or-required" :reason "常驻 Lisp 会话需要上下文连续, 但每个 wave 至少要落一次 durable commit")
             (exploration-only           :commit "not-required" :reason "只读调查无代码成果, complete 记录 findings 即可")
             (emergency-fix              :commit "required-after-verify" :reason "修正任务必须可回滚"))
          :anti-patterns ["不要 git add ."
                          "不要 stage 未 claim 文件"
                          "不要用 stash/reset/checkout 处理别人的 scope"
                          "不要只有 completion report 没有 commit_hash 就宣称代码交付完成"
                          "不要把 commit 当 coordination log — commit message 不替代 decisions/deviations"])

        ;; ── 设计约束 ────────────────────────────────────────
        (invariants
          :inv-1 "ID 只能由 manager 分配, 不允许人工手写下一个编号"
          :inv-2 "claim 不是永久锁, 必须带 lease + heartbeat + reap"
          :inv-3 "所有 write 必须做 paren-balance 与 schema-shape 校验"
          :inv-4 "status 读取优先走 derived-indexes, 但 audit 以正文 slot 为真"
          :inv-5 "deviation 不等于立即改 frozen design; 仍需指挥官批准"
          :inv-6 "execution = operational state; methodology = reusable playbook; 二者不得混写"
          :inv-7 "scoped commit 只能包含 claimed scope; commit_hash 必须回填 execution completion, 否则 status/audit 视为 durability gap")

        (placement-rationale
          :why-board
            ["board 是任务编排中心"
             "execution 实例本质是一次多 agent 协作任务"
             "phase / claim / completion 天然可与 board_task 或 flow_context 联动"]
          :why-not-intent-layer
            ["intent-layer 管 prescriptive 规约: 应该怎么做"
             "execution log 管 operational 实况: 这次实际怎么做"])))

    (module-egress
      (desc "MCP 查询 + 前端 WS 流 — 库对外的读出口")
      :principle "autopilot 的 tick-scan 是 engine 内部自读自写闭环 (见 pillar 二 2.4 :: autopilot), 非库的独立 reader"

      (reader mcp-board-query
        :binds-to [:mcp-surface]
        :tool "mission_board_query"
        :actions "list / get / search / summary / clear_done")

      (reader frontend-board-stream
        :binds-to [:frontend-surface]
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
        :binds-to [:mcp-surface]
        :tools  "mission_kb_mutate / mission_kb_remember / mission_kb_batch_set_project"
        :writes "knowledge (+ kb_operation_queue async)"
        :code   "daemon/src/handlers/knowledge/kb.rs")

      ;; ── 外部 worker 驱动的写入 (cross-ref pillar 二 2.3) ──
      (writer worker-arch-maintenance
        :binds-to [:worker-trait-surface]
        :cross-ref "pillar 二 2.3 :: workers/sonnet/arch_maintenance_worker.rs"
        :writes    "knowledge (category=architecture)"
        :library-pov "库暴露 KnowledgeStore write 接口 + category 约束; 扫描触发算法在 worker")

      (writer worker-experience-harvester
        :binds-to [:worker-trait-surface]
        :cross-ref "pillar 二 2.3 :: workers/local/experience_harvester.rs"
        :writes    "knowledge (category=bugfix/policy/memory)"
        :source    "module conversation-logs 的 conversations (跨模块 reader)"
        :library-pov "库暴露 write 接口; 挖掘策略 / 分类算法在 worker")

      (writer worker-tagger-chunker
        :binds-to [:worker-trait-surface]
        :cross-ref "pillar 二 2.3 :: workers/local/tagger_chunker.rs"
        :writes    "knowledge (分块 + 标签) + kb_ast_links"
        :library-pov "库暴露 knowledge + kb_ast_links 接口; 分块 + 打标算法在 worker")

      (writer worker-ast-sync
        :binds-to [:worker-trait-surface]
        :cross-ref "pillar 二 2.3 :: workers/local/ast_sync_worker.rs"
        :writes    "ast_nodes / ast_file_meta / beacons / beacon_nodes"
        :library-pov "库暴露 4 张代码索引表 + 增量 upsert 接口; 文件变更监听 + 解析在 worker")

      (writer worker-embedding-cross
        :binds-to [:worker-trait-surface]
        :cross-ref "pillar 二 2.3 :: workers/sonnet/embedding_worker.rs"
        :writes    "knowledge.embedding_vec + ast_nodes.embedding_vec"
        :cross-module "同 worker 也写 conv-logs (message_embeddings + topic_vectors) + project-mgmt (skill_topics)"
        :governance "cross-cutting :: capability embedding-storage-governance"
        :library-pov "库暴露 embedding_vec 列 + 类型约束; 生成调度 + provider 绑定在 worker")

      ;; ── daemon 内部 context-pipeline 审计写入 ──
      (writer context-pipeline-kb-audit
        :binds-to [:worker-trait-surface]
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
        :binds-to [:mcp-surface]
        :tools "mission_kb_query / mission_kb_search / mission_kb_ops"
        :reads "knowledge + knowledge_edges"
        :invokes "pillar 二 2.6 search-engines")

      (reader mcp-insight-recall
        :binds-to [:mcp-surface]
        :tools "mission_insight / mission_memory"
        :focus "综合洞察 / 记忆召回"
        :reads "knowledge")

      (reader mcp-code-search
        :binds-to [:mcp-surface]
        :tool "mission_code_search"
        :reads "knowledge + ast_nodes + beacons + kb_ast_links"
        :focus "代码语义 + 结构并查"
        :invokes "pillar 二 2.6 search-engines")

      (reader mcp-universe-graph
        :binds-to [:mcp-surface]
        :tool "mission_universe_graph"
        :reads "knowledge + knowledge_edges (跨项目)"
        :focus "实体 / 关系图生成")

      ;; ── 外部 compute 消费者 (cross-ref, 非库独立 reader) ──
      (reader context-pipeline-retrieval
        :binds-to [:worker-trait-surface]
        :cross-ref "pillar 二 context-pipeline (daemon 内部)"
        :reads "knowledge (向量 + 最近)"
        :purpose "为 LLM 调用拼 prompt"
        :library-pov "库暴露 search + retrieval 接口; token 预算 / 多源打分 由 pipeline 定"
        :note "记忆最密集的消费者 — 每次 LLM 调用都触发")

      (reader worker-code-prefetch
        :binds-to [:worker-trait-surface]
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
        :binds-to [:mcp-surface]
        :tools  "mission_conversation_reconcile / mission_conversation_analyze"
        :writes "conversations / conversation_messages"
        :code   "daemon/src/handlers/comm/conversation.rs")

      ;; ── 三引擎原始摄入 worker (cross-ref pillar 二 2.3 local) ──
      (writer worker-conversation-logger
        :binds-to [:worker-trait-surface]
        :cross-ref "pillar 二 2.3 :: workers/local/conversation_logger.rs"
        :writes    "conversations + conversation_messages + conversation_turns + conversation_events"
        :library-pov "库暴露 ConversationStore append 接口 + (session_id, turn_id) 幂等; tail JSONL 算法在 worker")

      (writer worker-codex-ingestion
        :binds-to [:worker-trait-surface]
        :cross-ref "pillar 二 2.3 :: workers/local/codex_ingestion_worker.rs"
        :writes    "conversations / conversation_messages (engine_type=codex)")

      (writer worker-gemini-logger
        :binds-to [:worker-trait-surface]
        :cross-ref "pillar 二 2.3 :: workers/local/gemini_logger.rs"
        :writes    "conversations / conversation_messages (engine_type=gemini)")

      (writer worker-gemini-reconcile
        :binds-to [:worker-trait-surface]
        :cross-ref "pillar 二 2.3 :: workers/local/gemini_reconcile_worker.rs"
        :writes    "conversations (对账校正)")

      (writer worker-conversation-organizer
        :binds-to [:worker-trait-surface]
        :cross-ref "pillar 二 2.3 :: workers/local/conversation_organizer.rs"
        :writes    "conversation_turns / conversation_tool_calls")

      ;; ── 派生分析 worker (cross-ref pillar 二 2.3 sonnet/codex) ──
      (writer worker-embedding
        :binds-to [:worker-trait-surface]
        :cross-ref "pillar 二 2.3 :: workers/sonnet/embedding_worker.rs"
        :writes    "message_embeddings (halfvec) / conversation_topic_vectors"
        :cross-module "同一 worker 也写 kb-manager (knowledge.embedding_vec / ast_nodes) + project-management (skill_topics)"
        :governance "cross-cutting :: capability embedding-storage-governance"
        :library-pov "库暴露 embedding_vec 列 + halfvec/vector 类型约束; 生成算法在 worker")

      ;; worker-step-narrator 已随 narration 移除 (v0.4.12), 见下方 writer-removed

      (writer worker-translation
        :binds-to [:worker-trait-surface]
        :cross-ref "pillar 二 2.3 :: workers/sonnet/translation_worker.rs"
        :writes    "message_translations")

      (writer worker-retro
        :binds-to [:worker-trait-surface]
        :cross-ref "pillar 二 2.3 :: workers/sonnet/retro_worker.rs"
        :writes    "retrospective_results"
        :trigger   "会话结束信号 / 手动触发 (具体时机由 worker 定)")

      ;; ── user_intents 写入 (v0.4.16 新增: 之前错列在 pillar 五) ──
      (writer worker-intent-analyst
        :binds-to [:worker-trait-surface]
        :cross-ref "pillar 二 :: engine/learning_engine/intent_analyst.rs"
        :writes    "user_intents + conversation_turns.intent_group_id (back-reference)"
        :mechanism "按 session 扫 conversation_turns, LLM 分析 turn 范围的意图, 增量游标式处理"
        :trait     "ConversationStore (insert_user_intent / update_turns_intent_group — trait 挂此, 因 trait 按 .rs 文件切不按业务切)"
        :library-pov "库暴露 insert + update_turns_intent_group; LLM 分析 / 游标策略 / 批次大小 / 置信度阈值 在 learning engine"
        :v0.4.16-note "之前 lisp 错列为 pillar 五 action-instruction-specs; 本条修正")

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
        :binds-to [:mcp-surface]
        :tools "mission_conversation_query / mission_conversation_analyze"
        :actions "get / list / search"
        :reads "conversations + messages + turns + events"
        :code "daemon/src/handlers/comm/conversation.rs"
        :note "v0.4.12: 不再读 narrations (表下线)")

      (reader mcp-retrospective-view
        :binds-to [:mcp-surface]
        :tool "mission_retrospective_manage"
        :actions "get / list"
        :reads "retrospective_results")

      (reader mcp-audit
        :binds-to [:mcp-surface]
        :tool "mission_audit"
        :reads "conversation_tool_calls + conversation_events")

      (reader mcp-llm-trace
        :binds-to [:mcp-surface]
        :tool "mission_llm_trace"
        :reads "conversation_tool_calls (+ category system-support :: gemini_requests)")

      ;; ── 前端 WS 流 ──
      (reader frontend-conversation-stream
        :binds-to [:frontend-surface]
        :source "pillar 四 event_log / DB watch"
        :emits "新 messages (v0.4.12: 不再推 narrations)"
        :via "daemon/src/bus/ws_bridge.rs")

      ;; ── 外部 compute 消费者 (cross-ref, 非库独立 reader) ──
      (reader context-pipeline-history
        :binds-to [:worker-trait-surface]
        :cross-ref "pillar 二 context-pipeline (daemon 内部)"
        :reads "最近 messages (v0.4.12: 不再读 narrations)"
        :purpose "每次 LLM 调用拼 prompt 时触发"
        :library-pov "库暴露 ConversationStore 查询接口; pipeline 策略 / token 预算在 daemon")

      (reader harvester-for-kb
        :binds-to [:worker-trait-surface]
        :cross-ref "module kb-manager :: worker-experience-harvester"
        :reads "conversations"
        :writes-to "knowledge (经 kb-manager ingress)"
        :note "跨模块链路: conversations 是 KB 语料库源头; 本条仅表明 '此 worker 读我的表'")

      ;; ── user_intents 读取 (v0.4.16 新增) ──
      (reader intent-analyst-self
        :binds-to [:worker-trait-surface]
        :cross-ref "pillar 二 :: engine/learning_engine/intent_analyst.rs"
        :reads "user_intents (游标计算: get_intent_coverage / sessions_pending_intent_analysis)"
        :purpose "增量处理 — 跳过已分析的 turn 范围, 只处理新 turn"
        :library-pov "库暴露 get_intent_coverage / sessions_pending_intent_analysis; 回填 / 增量策略在 learning engine")

      (reader autopilot-recent-intents
        :binds-to [:worker-trait-surface]
        :cross-ref "pillar 二 :: engine/intent_engine/autopilot.rs :: get_recent_intents caller (tick-time intent lookup)"
        :reads "user_intents (get_recent_intents 1800s 时窗)"
        :purpose "autopilot tick 判断时读近 30min 的用户意图, 辅助决策"
        :library-pov "库暴露 get_recent_intents(since_secs); 时窗 / 调用频率在 autopilot"))

    (module-tables-owned
      (desc "本模块独占 15 张表 (13 active + 2 pending-drop)")
      (tables conversations conversation_messages conversation_turns conversation_events
              conversation_tool_calls conversation_topic_vectors conversation_labels
              message_embeddings message_embedding_skips
              message_narrations narration_cursors
              message_translations message_labels
              retrospective_results
              user_intents)
      (count 15)
      (active-count 13)
      (pending-drop
        (message_narrations :status "pending-drop-v0.4.12" :reason "briefing-worker (v1.3.0 删) + step-narrator (v0.4.12 删) 已无 writer, migration 待写"  :施工-action "确认无 writer/reader 后写 drop migration")
        (narration_cursors  :status "pending-drop-v0.4.12" :reason "同上, 仅 narration 防重游标"  :施工-action "同上, 一起 drop"))
      (removed-in-v0.4-revision "image_descriptions 挪到 module system-support (独立图片缓存, 无外键, 非 conversation-scoped)")
      (added-in-v0.4.16 "user_intents (之前错列在 pillar 五 action-instruction-specs; 实际 writer=intent_analyst, reader=autopilot+self, trait 挂 ConversationStore)")
      (v0.4.22-count-correction "原 count 13 是 '预期清理后' 数量, 但 narrations 2 张 migration 未写实际仍在; 改为 count 15 实况 + active-count 13 + pending-drop 2")
      (non-db-forms-owned
        (jsonl-source "~/.claude/projects/{encoded}/*.jsonl (see non-db-forms :: external-source-streams)")))

    (cross-module-trait-sharing
      (desc "本 module 的 trait 跨模块共享关系 — 主 trait 独占, 消费部分共享 trait")
      :primary-traits
        ("ConversationStore (db/traits.rs :: ConversationStore trait body) — 本 module 主 trait, 36+ 方法覆盖 conversations/messages/turns/events/user_intents/labels"
         "MessageStore (db/traits.rs :: MessageStore trait body) — 消息级细粒度 CRUD + 搜索 + 分页")
      :consumes-shared-trait
        ("ObservabilityStore (db/traits.rs :: ObservabilityStore trait body) 的 label_* + conversations_missing_summary_* 方法 — trait 挂 observability.rs 文件但语义归本模块消费"
         "SlotStore 的极少方法 (slot_id 反查场景, 主归 slot-support)")
      :design-note "ConversationStore/MessageStore 和本 module 1:1 无阻抗; ObservabilityStore/SlotStore 跨 module 是 trait 按 .rs 文件切分产生的阻抗不匹配, 接受 + 显式披露"
      :施工-note "本模块代码同构时, ConversationStore/MessageStore 全部 impl 在本 module 范围; ObservabilityStore 的 label_* 方法 impl 在 db/pg/observability.rs 但逻辑按本 module 约束"))


  ;; ═════════════════════════════════════════════════════════════
  ;;  业务模块 5: Directive-Layer Store (directive → plan → workflow 三段编译 pipeline)
  ;;  store+manager code-aligned partial — writer actors 待 pillar 五 actor
  ;; ═════════════════════════════════════════════════════════════
  (module directive-layer
    (desc "user utterance → lisp 指令(directive) → executable plan DAG → reusable workflow template 三段式编译 pipeline 的持久化层")
    (maturity "store+manager code-aligned partial — migration + DirectiveLayerStore + MCP manager surfaces 已就位; LLM compiler/distiller actors 待启用")
    :created "v0.4.17 (2026-04-19)"
    :rationale "v0.4.4 原计划迁 pillar 五, v0.4.16 误判为 dead, v0.4.17 用户澄清是'刚建未启用'的预留 schema; 按 memory=库 原则, 先把 schema+trait 接口立起来, writer/reader 留 TBD cross-ref"

    (primary-goals
      (goal-1 directive-compilation-audit
        :problem  "LLM 对用户 utterance 编译出的 lisp 指令(directive) 需要版本追踪 + 状态 FSM + 审阅痕迹 + compiler_model 元数据"
        :solution "directive 表: utterance_text / sexp_text / version / status (draft|refining|approved|compiled|archived) / compiler_model / references_json"
        :benefit  "每次 directive 编译可追溯; status 机控制可见性; UNIQUE(id, version) 保证版本不冲突")
      (goal-2 plan-persistence
        :problem  "directive 编译出的执行 DAG 需要和 board_task 绑定, 支持 approval gate + 失败回滚 + 多版本迭代"
        :solution "plan 表: board_task_id FK → board_tasks + source_directive_id FK → directive + sexp_text/hash + version + FSM (draft→awaiting_approval→approved→executing→succeeded/failed/superseded)"
        :benefit  "每个 board_task 有多版本 plan (UNIQUE(board_task_id, version)); FK 级联删除; compiled_from 追溯 directive sexp_hash 血统")
      (goal-3 workflow-distillation
        :problem  "成功的 plan 应该沉淀为可复用的模板; 下次匹配到类似 utterance 可直接套用"
        :solution "workflow 表: name UNIQUE + sexp_text + match_rules JSONB + learned_from FK → plan + executions/success_count/avg_cost_usd/last_used_at"
        :benefit  "plan → workflow 蒸馏闭环; LRU + 成功率指标支持 workflow 推荐"))

    (file-first-artifacts
      :status "code-aligned partial — sexp / DB row 写入由 actor v0 代码同构 (directive-compiler / plan-compiler / workflow-distiller / methodology compiler / plan-runner); 但 file-first .missiond/alignment/*.lisp / .missiond/plans/*.lisp / .missiond/workflows/*.lisp 自动写入/同步仍 code-alignment pending — 当前由 ClaudeCode 主会话/外部 LLM 手工产出文件, DB row 由 actor v0 写"
      :rationale "代码同构阶段需要人类/Codex 可审阅的文件产物先行; DB 三表作为可查询镜像,不强迫第一版就全自动写库"
      :unified-pipeline-anchor "flow pillar :: F-intent-alignment-plan-execution-loop (canonical message → alignment → plan → execution → workflow) + intent-layer pillar :: section unified-entry-pipeline"
      :ssot-policy "file-first SSOT — alignment/plan/workflow .lisp 是真正 review 边界; directive/plan/workflow 表是可查询镜像 + 状态管理面 (mission_directive/mission_plan/mission_workflow)"

      (artifact intent-alignment-lisp
        :path ".missiond/alignment/<topic>/intent-alignment.lisp"
        :maps-to "directive table (DB 镜像; 通过 mission_directive(action=compile, compiler_mode=sonnet, persist=true) 落 draft / 通过 approve/archive 流转)"
        :purpose "本轮架构变更或 message 的目标/边界/非目标/验收条件/涉及 pillar"
        :status-lifecycle "draft → reviewing → approved | rejected | superseded"
        :review-gate "alignment-review-gate (intent-layer section unified-entry-pipeline)"
        :review-gate-owner "human/Codex"
        :gate-rule "未通过 approval 不允许进入 plan-authoring 阶段"
        :writer "directive-compiler actor v0 (compiler_mode=sonnet) — 写 directive sexp + DB draft; file 路径自动写入仍 pending"
        :code-aligned-writes ["directive sexp + references_json + directive table draft (mission_directive compiler_mode=sonnet, persist=true)"]
        :pending-writes [".missiond/alignment/<topic>/intent-alignment.lisp 自动写入/与 directive 表双向同步"]
        :manager-surface "mission_directive (compile / approve / archive / version_chain / list / get)")

      (artifact plan-lisp
        :path ".missiond/plans/<topic>/PLAN.lisp"
        :maps-to "plan table (DB 镜像; 通过 mission_plan(action=compile, compiler_mode=sonnet, persist=true) 落 draft/awaiting_approval / 通过 approve/mark/supersede 流转)"
        :purpose "LLM 规划 + human/Codex review 后的可执行计划,含 files/phases/tasks/tests/risks/rollback"
        :status-lifecycle "draft → reviewing → approved → executing → succeeded | failed | superseded"
        :review-gate "plan-review-gate (intent-layer section unified-entry-pipeline)"
        :review-gate-owner "human/Codex"
        :gate-rule "未通过 approval 不允许进入 execution-runner 阶段"
        :writer "plan-compiler actor v0 (compiler_mode=sonnet) — 写 plan sexp + DB awaiting_approval; file 路径自动写入仍 pending"
        :code-aligned-writes ["plan sexp + plan table draft/awaiting_approval (mission_plan compiler_mode=sonnet, persist=true)"]
        :pending-writes [".missiond/plans/<topic>/PLAN.lisp 自动写入/与 plan 表双向同步"]
        :manager-surface "mission_plan (compile / approve / mark / supersede / execute / record_evidence / list / get / by_task)")

      (artifact workflow-lisp
        :path ".missiond/workflows/<topic>.lisp"
        :maps-to "workflow table (DB 镜像; 通过 mission_workflow(action=distill, distill_mode=sonnet, persist=true) 落 draft/template / 通过 record_execution 累计统计)"
        :purpose "多次成功执行后的可复用方法论; 可继续走 F-methodology-to-executable-compile 编译为 YAML 由 flow-engine-v2 runner 执行"
        :status-lifecycle "draft → published → deprecated"
        :created-when "成功 plan 多次重复 或 human 显式标 reusable"
        :writer "workflow-distiller actor v0 (distill_mode=sonnet) — 写 workflow sexp + match_rules + DB draft/template; file 路径自动写入仍 pending"
        :code-aligned-writes ["workflow sexp + match_rules JSON + workflow table draft/template (mission_workflow distill_mode=sonnet, persist=true)" "<project_root>/.missiond/generated/flows/<flow_id>.yaml (compile_mode=deterministic, persist=true)"]
        :pending-writes [".missiond/workflows/<topic>.lisp 自动写入/与 workflow 表双向同步"]
        :manager-surface "mission_workflow (distill / record_execution / compile_methodology / run_methodology / match / apply / list / get)")

      (artifact plan-evidence-sidecar
        :path ".missiond/v2/plans/<plan_id>.evidence.json"
        :maps-to "未来可升级为 plan_evidence DB JSONB 列或独立表; 当前为 file-only sidecar"
        :purpose "plan 执行证据落盘 — git diff / tests / tool_calls refs / event_log refs / execution companion log refs / deviations / decisions / completions / plan_runner_dispatch entries"
        :writer "evidence-collector role — mission_plan(action=record_evidence) 显式写入 + plan-runner v0 internal mode 自动追加 plan_runner_dispatch entry (含 dispatch_strategy / target_project / requested_cwd / target / inner result)"
        :status "code-aligned partial — sidecar 写入 code-aligned (record_evidence + plan-runner internal append); 全自动 evidence-collector actor (聚合 git diff / event_log / ExecutionEvent / test outputs / 跨执行路径) 仍 code-alignment pending; 升级到 plan_evidence DB JSONB 仍 pending"
        :consumer "workflow-distiller (s8) + retrospective + capability-usage-monitor (间接)"
        :ssot-note "当前文件即真相; 升级为 DB 时保留文件作为长期归档"
        :dag-scheduler-future "actor plan-dag-scheduler (intent-layer architecture-designed) 升级时, sidecar 增 nodes[<node_id>] block (per-node attempts / claim_id / start-end / inner_result / acceptance_pass / rollback_path); v0 plan_runner_dispatch entry shape 必须向后兼容")

      (artifact plan-node-state-projection
        :path ".missiond/v2/plans/<plan_id>.evidence.json :: nodes[<node_id>] (sidecar 子树, 与 plan-evidence-sidecar 同文件)"
        :status architecture-designed
        :purpose "完整 PLAN DAG scheduler 的 per-node 状态 + evidence 聚合视图 — 当前 v0 单节点 dispatch 不需要; v1 多节点 DAG 启用后必须落"
        :scheduler-cross-ref "intent-layer pillar :: section action-instruction-actor :: actor plan-dag-scheduler"
        :flow-cross-ref "flow pillar :: F-intent-alignment-plan-execution-loop :: s6 :: dag-scheduler"
        :node-fsm-enum [pending ready claimed running succeeded failed skipped retrying rolling-back paused]
        :per-node-block-shape
          ((node_id        :type "string" :desc "PLAN.lisp 节点 :id (kebab-case)")
           (state          :type "enum"   :desc "node-fsm-enum 当前态")
           (claim_id       :type "string" :desc "由 mission_execution(action=claim) 原子分配, 不允许 scheduler 自建")
           (lease_expires_at :type "iso8601" :desc "claim lease 截止")
           (target         :type "enum"   :desc "本次 dispatch substrate")
           (dispatch_strategy :type "enum" :desc "本次解析后的策略")
           (target_project :type "string" :desc "解析后的 target_project_root")
           (requested_cwd  :type "path"   :desc "解析后的 cwd, 必须在 target_project 内")
           (attempts       :type "list"   :desc "[{attempt_no, started_at, ended_at, inner_result, acceptance_pass, error?}] — 每次 retry 一个条目")
           (acceptance_pass :type "bool"  :desc "节点 :acceptance 验证结果; 缺省以 inner_result success 为准")
           (rollback_path  :type "list"   :desc "若触发 rollback, 记录补偿节点 chain")
           (artifacts_refs :type "list"   :desc "execution companion log claim/completion ref / git diff path / test output ref"))
        :writer-future "actor plan-dag-scheduler s7 collect-node-evidence + s8 update-node-state 落; v0 不写本块, 仅写顶层 plan_runner_dispatch entry"
        :consumer-future "workflow-distiller s8 (per-node 模式提取) + capability-usage-monitor (per-node retry / dispatch_strategy 命中率) + retrospective"
        :ssot-rule "本块仍是 file-first sidecar; 升级到 DB plan_evidence JSONB 时保留 sidecar 长期归档 — 与 plan-evidence-sidecar 共享 ssot-note"
        :pending-writes ["actor plan-dag-scheduler 实现 (crates/missiond-daemon/src/intent_layer/plan_runner.rs + plan_dag.rs)"
                         "ExecutionEvent::PlanNodeStateChanged 事件扩展 (per-node FSM 转移广播)"]))

    (module-ingress
      (desc "写入 3 张表的路径 — MCP 管理面已启用, LLM writer actors 待 pillar 五 实现")
      :principle "memory = 库. 本模块声明 schema + trait 接口形态; 管理面可做 draft/control 写入, utterance→directive / directive→plan / plan→workflow 的智能编译仍在 actor/worker 侧"
      :status-note "ingress 当前为 MCP 管理面 + 未来 actor 组合:"
      :option-A "pillar 二 新 worker (类似 intent_analyst 之于 user_intents)"
      :option-B "pillar 五 intent-layer actor (目前 pillar 五 只有 intent-files/graph/forge/governance 元层组件, 需新增 actor)"
      :option-C "MCP 工具直写 (mission_directive / mission_plan / mission_workflow 三族, code-aligned partial)"

      ;; ── writer v0 (commits 0a3ffe0 / 46a9453 / 3ed14fc + working-tree parallels) ──
      (writer directive-compiler-v0
        :binds-to [:mcp-surface]
        :status "code-aligned partial — mission_directive(action=compile, compiler_mode=sonnet) 走 SonnetGateway interactive lane 编译 directive sexp + 校验 fenced block / parens / allowed top-level head; persist=true 写 draft row; auto multi-round refine + file 自动写入仍 pending"
        :writes "directive (create draft / update status / set approved_at / set compiler_model = 'claude-sonnet')"
        :triggers ["mission_directive(action=compile, compiler_mode=sonnet) MCP 调用"]
        :api-used "DirectiveLayerStore::directive_insert / directive_update_status / directive_approve"
        :implementation-targets ["crates/missiond-daemon/src/handlers/knowledge/directive.rs (action=compile sonnet branch)"
                                 "crates/missiond-daemon/src/llm/sonnet_gateway.rs"]
        :pending ["alignment file (.missiond/alignment/<topic>/intent-alignment.lisp) 自动写入"
                  "compiler 自主多轮 refine"
                  "auto QuestionEvent gate"])

      (writer plan-compiler-v0
        :binds-to [:mcp-surface]
        :status "code-aligned partial — mission_plan(action=compile, compiler_mode=sonnet) 从 approved directive / board task context 编译 plan sexp DAG/FSM; compiled_from = directive/<id>:<version> 或 board_task/<id>; persist=true 写 awaiting_approval (不自动 approve); planner-class model alias 切换 + file 自动写入仍 pending"
        :writes "plan (create from directive / update status / set executing/succeeded/failed / superseded-chain)"
        :triggers ["mission_plan(action=compile, compiler_mode=sonnet) MCP 调用"]
        :api-used "DirectiveLayerStore::plan_insert / plan_update_status / plan_supersede"
        :implementation-targets ["crates/missiond-daemon/src/handlers/knowledge/plan.rs (action=compile sonnet branch)"
                                 "crates/missiond-daemon/src/llm/sonnet_gateway.rs"]
        :pending ["PLAN file (.missiond/plans/<topic>/PLAN.lisp) 自动写入/同步"
                  "planner-class model alias 切换 (例: opus planner)"
                  "alignment 多轮 refine 后增量 re-compile"])

      (writer plan-runner-v0
        :binds-to [:mcp-surface]
        :status "code-aligned partial — mission_plan(action=execute, execute_mode=internal) 直接 dispatch 到 mission_execution / mission_task_delegate / mission_flow_run + 写 evidence sidecar plan_runner_dispatch entry + 推 plan FSM 到 executing (单节点 dispatch); auto-selection v1 从 plan.sexp_text 保守解析 :target / :dispatch-strategy / :parallelism / :target-project / :requested-cwd 等 hints (explicit args 优先, 无法安全推断时 MISSING_PARAM); 完整 PLAN DAG scheduler / 多节点 dependency / arbitrary semantic interpretation 仍 architecture-designed pending — 详 intent-layer pillar :: actor plan-dag-scheduler"
        :writes "plan FSM (status → executing) + .missiond/v2/plans/<plan_id>.evidence.json (plan_runner_dispatch entry)"
        :triggers ["mission_plan(action=execute, execute_mode=internal) MCP 调用"]
        :implementation-targets ["crates/missiond-daemon/src/handlers/knowledge/plan.rs (action=execute internal + auto-selection v1)"
                                 "crates/missiond-daemon/src/handlers/knowledge/agent_execution.rs (mission_execution forwarding)"
                                 "crates/missiond-daemon/src/handlers/compute/task_delegate.rs (mission_task_delegate forwarding)"]
        :v1-future-target "writer plan-dag-scheduler — actor plan-dag-scheduler (intent-layer architecture-designed) 实现后, 此 writer 升级为 DAG 调度循环 + per-node FSM + claim/lease 接入 (复用 helper agent-execution-coordination); v0 单节点 fast-path 必须保留"
        :pending ["完整 PLAN DAG scheduler / 多节点 dependency / 并发 dispatch / per-node retry-failure-policy / condition-gate / rollback-compensation / per-node evidence aggregation (architecture-designed; intent-layer pillar :: actor plan-dag-scheduler)"
                  "arbitrary PLAN.lisp 语义解释 (超出保守 key/value hints)"
                  "PLAN file (.missiond/plans/<topic>/PLAN.lisp) 自动写入"
                  "auto QuestionEvent gate"
                  "ExecutionEvent dispatch metadata 扩展 (含 PlanNodeStateChanged 字段)"
                  "status update failure 时事务性回滚"])

      (writer workflow-distiller-v0
        :binds-to [:mcp-surface]
        :status "code-aligned partial — mission_workflow(action=distill, distill_mode=sonnet) 读 succeeded plan + evidence sidecar 生成 workflow sexp + match_rules JSON; persist=true 写 draft/template; methodology compiler v0 (compile_mode=deterministic) 已实现 paren-validate + (step ...) 抽取 + executable YAML; generated flow loader code-aligned partial; 高阶 semantic lifting / forge compiler / 自动 record_execution-distill 联动 / file 自动写入仍 pending"
        :writes "workflow (create from successful plan / 累加 executions+success_count / update last_used_at / upsert match_rules) + <project_root>/.missiond/generated/flows/<flow_id>.yaml"
        :triggers ["mission_workflow(action=distill, distill_mode=sonnet) MCP 调用" "mission_workflow(action=compile_methodology, compile_mode=deterministic) MCP 调用"]
        :api-used "DirectiveLayerStore::workflow_insert / workflow_record_execution / workflow_find_by_match"
        :implementation-targets ["crates/missiond-daemon/src/handlers/knowledge/workflow.rs (action=distill sonnet + action_compile_methodology deterministic + action_run_methodology)"
                                 "crates/missiond-daemon/src/engine/flow/loader.rs (generated flow loader)"
                                 "crates/missiond-daemon/src/handlers/compute/flow_run.rs (mission_flow_run discoverability)"]
        :pending ["workflow file (.missiond/workflows/<topic>.lisp) 自动写入/同步"
                  "distiller 高阶 semantic lifting (phase / anti-pattern / authority)"
                  "forge compiler 接管 methodology → flow 编译"
                  "成功 plan 自动触发 distill"
                  "record_execution 与 distill 双向自动联动"
                  "longest-prefix cwd resolver"]))

    (module-core
      (desc "3 张表 schema + trait 接口 — 三段式 pipeline 的持久化约束")
      :trait "DirectiveLayerStore (code-aligned in crates/missiond-core/src/db/traits.rs; PG impl in crates/missiond-core/src/db/pg/directive.rs; 独立 trait 不和 ConversationStore 混)"
      :migration "crates/missiond-core/migrations/20260420000000_directive_plan_workflow.sql"

      (plumbing directive-compilation
        (desc "user utterance → sexp 编译记录 + 版本/状态追踪")
        :table "directive"
        :schema-cols "id UUID PK / utterance_text / sexp_text / version INT / status TEXT / compiler_model / references_json JSONB / created_at / approved_at"
        :fsm "draft → refining → approved → compiled → archived"
        :invariant "UNIQUE(id, version) — 同一 directive 多版本演进保留历史"
        :expected-trait-methods "directive_insert / directive_get / directive_update_status / directive_approve / directive_list_by_status / directive_get_version_chain / directive_list_recent"
        :scoping-candidate "加 project_id 列 — 项目级指令隔离")

      (plumbing plan-execution
        (desc "directive 编译出的执行 DAG — 绑 board_task + 版本 + FSM")
        :table "plan"
        :schema-cols "id UUID PK / board_task_id TEXT FK→board_tasks / source_directive_id UUID FK→directive / version INT / sexp_text / sexp_hash / status TEXT / compiler_model / compiled_from TEXT / created_at / approved_at / finished_at"
        :fsm "draft → awaiting_approval → approved → executing → succeeded / failed / superseded"
        :invariants "UNIQUE(board_task_id, version); ON DELETE CASCADE from board_tasks; compiled_from = 源 directive 的 sexp_hash (血统追溯)"
        :expected-trait-methods "plan_insert / plan_get / plan_update_status / plan_supersede / plan_list_by_task / plan_get_latest / plan_list_recent"
        :cross-module-note "board_tasks.source_directive_id 列 (v0.4.4 ALTER TABLE) 是 board → directive 的反向指针; 见 cross-module-invariants")

      (plumbing workflow-templates
        (desc "从成功 plan 蒸馏的可复用模板 — 带匹配规则 + 执行统计")
        :table "workflow"
        :schema-cols "id UUID PK / name UNIQUE / sexp_text / match_rules JSONB / learned_from UUID FK→plan / executions INT / success_count INT / avg_cost_usd NUMERIC(10,4) / last_used_at / created_at"
        :invariant "name UNIQUE; learned_from 可为 NULL (手写模板); executions/success_count 单调递增"
        :expected-trait-methods "workflow_insert / workflow_get_by_name / workflow_get_by_id / workflow_find_by_match / workflow_record_execution / workflow_list_top_n"
        :scoping-candidate "加 project_id 列 — 项目私有 workflow 库 (区别全局模板)")

      ;; ── 跨模块约束 ──
      (cross-module-invariants
        (invariant board_tasks-reverse-pointer
          :desc "board_tasks.source_directive_id UUID FK→directive(id) 是 board module 拥有的列, 但指向本 module 的 directive 表"
          :added-by-migration "同一 migration 20260420000000 的 ALTER TABLE"
          :owner-of-column "module board (board_tasks 表)"
          :owner-of-target "module directive-layer (directive 表)"
          :semantic "board_task 记录其 originating directive; 允许反向查询'这个 task 源自哪次 utterance'"
          :impact "board module 若 drop source_directive_id 需通知本 module; 本 module drop directive 会 ON DELETE 影响 board_tasks (但 FK 是 NULL-able, CASCADE 未声明)")

        (invariant plan-chain-provenance
          :desc "plan.compiled_from (TEXT sexp_hash) vs plan.source_directive_id (UUID FK) 双重血统追溯"
          :semantic "UUID FK 给查询用, sexp_hash 给防 directive 变更时旧 plan 仍保留血统快照; 即 directive 改版后 plan 仍知自己来源哪个 hash")))

    (module-egress
      (desc "读取 3 张表 — MCP 管理面已实现, autopilot/actor consumer 仍待接入")
      :principle "memory = 库. egress 暴露查询原语, 消费策略在外"

      (reader mcp-directive
        :binds-to [:mcp-surface]
        :status "code-aligned partial — read/control + compile compiler_mode=sonnet (writer directive-compiler-v0); auto QuestionEvent + file-first writer 仍 pending"
        :expected-tool "mission_directive (action=list / get / version_chain / approve / archive / compile {dry_run|sonnet})"
        :expected-reads "directive (按 status / version / utterance 模糊查)"
        :expected-api "DirectiveLayerStore::directive_list_by_status / directive_list_recent / directive_get_version_chain")

      (reader mcp-plan
        :binds-to [:mcp-surface]
        :status "code-aligned partial — read/control + compile compiler_mode=sonnet + execute_mode=internal (plan-runner v0 + auto-selection v1) + record_evidence; PLAN DAG scheduler / file-first writer 仍 pending"
        :expected-tool "mission_plan (action=list / get / by_task / approve / mark / supersede / execute {bridge|internal} / record_evidence / compile {dry_run|sonnet})"
        :expected-reads "plan (按 board_task_id 查版本链 / 按 status 查 awaiting_approval / 按 sexp_hash 查 provenance)"
        :expected-api "DirectiveLayerStore::plan_list_by_task / plan_list_recent / plan_get_latest")

      (reader mcp-workflow
        :binds-to [:mcp-surface]
        :status "code-aligned partial — read/control + distill distill_mode=sonnet + compile_methodology compile_mode=deterministic + run_methodology (复用 flow-engine-v2 runner path); semantic lifting / file-first writer 仍 pending"
        :expected-tool "mission_workflow (action=list / get / match / apply / distill {dry_run|sonnet} / record_execution / compile_methodology {dry_run|deterministic} / run_methodology)"
        :expected-reads "workflow (按 match_rules 查匹配 / 按 executions 排 top-n / 按 name get)"
        :expected-api "DirectiveLayerStore::workflow_get_by_id / workflow_find_by_match / workflow_list_top_n")

      (reader TBD-autopilot-workflow-match
        :binds-to [:worker-trait-surface]
        :status "未实现 (潜在 consumer)"
        :expected-cross-ref "pillar 二 :: engine/intent_engine/autopilot — tick 时拿 utterance 查 workflow 匹配"
        :expected-reads "workflow (match_rules JSONB 查询)"
        :library-pov "库暴露 workflow_find_by_match 原语; 匹配策略 / 阈值 / fallback 在 autopilot"))

    (module-tables-owned
      (desc "本模块独占 3 张表 + 1 个跨模块反向指针列")
      (tables directive plan workflow)
      (count 3)
      (cross-module-column "board_tasks.source_directive_id UUID FK→directive(id) — 列由 board module 持有, FK 目标归本 module")
      (schema-source "crates/missiond-core/migrations/20260420000000_directive_plan_workflow.sql")
      (non-db-forms-owned
        (per-project-intent-file "<project>/.missiond/intent.lisp — 注: 这是 memory :: project-management 管的文件, 记 factual 代码快照, 不是本 module; 本 module 的 directive 表是 instruction 规约 DB 镜像")
        (intent-alignment-file ".missiond/alignment/<topic>/intent-alignment.lisp — directive 表的 file-first source,用于代码同构前的人机对齐")
        (plan-file ".missiond/plans/<topic>/PLAN.lisp — plan 表的 file-first source,用于人类审核/修订后再执行")
        (workflow-methodology-file ".missiond/workflows/<topic>.lisp — workflow 表的 file-first source,多次成功后沉淀")
        (system-intent-files "pillar 五 :: component intent-files — .missiond/v2/*.lisp 系统级规约文件, 不是本 module; 但未来 forge 冲压链路可能把 lisp → directive 表做双向同步")))

    (cross-module-trait-sharing
      (desc "本 module 暂不与其他 trait 共享 — 建议 DirectiveLayerStore 独立 trait (不和 ConversationStore 混), 原因:")
      :reason-1 "3 张表语义是 instruction/prescription (应该做什么), conversation-logs 的 user_intents 是 fact (做过什么) — 分层原则"
      :reason-2 "未来可能按 .rs 文件切 trait (intent_store.rs / plan_store.rs / workflow_store.rs 各一个), 或合一个 intent_layer.rs"
      :future-decision "待 pillar 二/五 actor 设计时决定 trait 切分粒度")

    (pending-implementation-checklist
      (desc "启用本 module 需要的代码侧动作 — 按依赖顺序")
      "[done] 1. crates/missiond-core/src/db/traits.rs 新增 trait DirectiveLayerStore (含 recent/get_by_id manager 方法)"
      "[done] 2. crates/missiond-core/src/db/pg/directive.rs PG impl 扩展 directive_list_recent / plan_list_recent / workflow_get_by_id"
      "[done] 3. crates/missiond-core/src/types/ 已有 Directive / Plan / Workflow struct + status enum"
      "[done-v0] 4. actor v0 已 code-aligned partial: directive-compiler v0 (compiler_mode=sonnet) / plan-compiler v0 (compiler_mode=sonnet) / plan-runner v0 + auto-selection v1 / workflow-distiller v0 (distill_mode=sonnet) / methodology compiler v0 (compile_mode=deterministic + run_methodology) / generated flow loader"
      "[done] 5. MCP 工具 mission_directive / mission_plan / mission_workflow 三族 handler: read/control/draft persistence/execute (bridge + internal)/record_evidence/compile/distill/compile_methodology/run_methodology 已实现"
      "[pending] 6. file-first writer/sync — alignment/PLAN/workflow .lisp 自动写入仍未实现"
      "[pending] 7. 高阶 semantic lifting / forge compiler / 完整 PLAN DAG scheduler / planner-class model alias / auto QuestionEvent gate / ExecutionEvent dispatch metadata"
      "[optional] 8. 加 project_id 列做项目级 scoping — 详见 candidates-for-promotion"))


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
        :binds-to [:worker-trait-surface]
        :cross-ref "pillar 二 2.3 :: workers/local/gemini_logger.rs"
        :mechanism "event-bus subscriber on LlmEvent; two-step persist:"
        :step-1    "gemini_log_insert_started (id + caller + model + prompt_chars + prompt_text)"
        :step-2    "gemini_log_update_completed (id + api_mode + response + duration_ms + status)"
        :writes    "gemini_requests"
        :startup   "开机清 7 天前旧日志 (gemini_log_cleanup)"
        :library-pov "库暴露 gemini_log_insert_started + _update_completed; event-bus 订阅 + 采样 / 清理 策略在 worker")

      (writer llm-gateways-file-cache
        :binds-to [:worker-trait-surface]
        :cross-ref "pillar 二 2.2 :: daemon/src/llm/gemini_file_api.rs"
        :writes    "gemini_file_uploads (file_hash 去重 upsert)"
        :api       "ObservabilityStore::gemini_file_cache_get / _put / _gc"
        :library-pov "库暴露 cache get/put/gc 三接口; 上传前查 cache miss 再调 Gemini API, TTL 决策在 gateway")

      (writer token-usage-accounting
        :binds-to [:worker-trait-surface]
        :cross-ref "crates/missiond-daemon/src/infra/message_handler.rs :: PTY JSONL 摄入下游 (grep "insert_token_usage") (PTY JSONL 摄入下游)"
        :trigger "conversation-logger 摄入时, 对含 usage 字段的消息调 insert_token_usage"
        :writes  "token_usage_ledger (每消息一条: conversation_id + slot_id + model + 4 个 tokens + message_id)"
        :api     "ObservabilityStore::insert_token_usage"
        :library-pov "库暴露 insert_token_usage + token_stats; 摄入侧决定 '何时写 / 挑哪些消息'"
        :correction-v0.4.13 "之前假设在 gateway wrapper / fanout subscriber 是错的; 实际是摄入层"))

    (module-core
      (desc "3 张 LLM 观测表 — schema + 语义; 统一通过 ObservabilityStore super-trait 访问")
      :trait "ObservabilityStore (crates/missiond-core/src/db/traits.rs :: ObservabilityStore trait body) — 注: 此 trait 还管 incidents / watermark / labels, 跨 llm-support / system-support / conversation-logs 多模块"

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
        :binds-to [:mcp-surface]
        :tool    "mission_llm_trace"
        :reads   "gemini_requests"
        :api     "ObservabilityStore::gemini_log_query / _stats / _get_content"
        :code    "daemon/src/handlers/comm/question.rs :: handle (注: 与 mission_question / decision_stats / incident / gemini_auth 共用 question.rs handler)"
        :actions "list / by-session / by-model / by-time")

      (reader mcp-cost-report
        :binds-to [:mcp-surface]
        :status  "🚧 MCP 未实现, 但底层查询原语已有"
        :tool    "mission_cost_report"
        :reads   "token_usage_ledger"
        :api     "ObservabilityStore::token_stats(conversation_id, slot_id, since, group_by) 已实现"
        :purpose "token 花销报表 — by model / by conversation / time-series / 未来按项目"
        :implementation-effort "低 — 只需写 MCP handler 包装 token_stats 查询, 核心 SQL 已在 trait 里")

      (reader timeline-enrichment
        :binds-to [:worker-trait-surface]
        :cross-ref "crates/missiond-daemon/src/handlers/comm/timeline.rs :: gemini_request_completed enrich"
        :reads "gemini_requests (用于 enrich gemini_request_completed 事件的 timeline 展示)"
        :api "ObservabilityStore::gemini_log_get_content(request_id)"
        :purpose "Timeline UI/CLI 显示 LLM 调用事件时, 从 gemini_requests 拉完整 prompt/response 细节")

      (reader daemon-stats-counter
        :binds-to [:worker-trait-surface]
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
      :shared-trait "ObservabilityStore (db/traits.rs :: ObservabilityStore trait body)"
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
        :binds-to [:worker-trait-surface]
        :cross-ref "pillar 二 2.1 :: daemon/src/slot_orchestrator/mod.rs"
        :writes    "slot_sessions (slot 启动时绑定 session)"
        :library-pov "库暴露 set_slot_session (upsert on conflict); orchestrator 决定何时绑定")

      (writer slot-env-bind
        :binds-to [:worker-trait-surface]
        :cross-ref "pillar 二 2.1 :: daemon/src/context/slot_env.rs"
        :writes    "slot_sessions (slot 环境构建时同步)")

      (writer ingestion-router-bind
        :binds-to [:worker-trait-surface]
        :cross-ref "daemon/src/infra/ingestion_router.rs"
        :writes    "slot_sessions (JSONL 摄入发现新 session 时更新)")

      ;; ── slot_tasks writers (learning engine 专属) ──
      (writer learning-engine-extraction
        :binds-to [:worker-trait-surface]
        :cross-ref "pillar 二 :: daemon/src/engine/learning_engine/extraction.rs (3 call sites)"
        :writes    "slot_tasks (task_type=extraction 类, 3 种场景)"
        :library-pov "库暴露 insert_slot_task + status 转换接口 (set_running / set_completed / set_failed); 触发 + 派发在 extraction engine")

      (writer learning-engine-decision
        :binds-to [:worker-trait-surface]
        :cross-ref "pillar 二 :: daemon/src/engine/learning_engine/decision_engine.rs"
        :writes    "slot_tasks (task_type=decision)")

      ;; ── dynamic_slots writers (MCP + 清理) ──
      (writer mcp-compute-slot
        :binds-to [:mcp-surface]
        :tools     "mission_compute_slot (actions: create / terminate / extend / list)"
        :code      "daemon/src/handlers/compute/compute_slot.rs"
        :writes    "dynamic_slots (create:216 / terminate:293,339 / extend:385)"
        :library-pov "库暴露 create/terminate/extend/list 接口 + (status, expires_at) 索引; MCP handler 做业务编排")

      (writer daemon-restart-cleanup
        :binds-to [:worker-trait-surface]
        :cross-ref "daemon/src/main.rs"
        :writes    "dynamic_slots (terminate on 'daemon_restart')"
        :library-pov "库暴露 terminate_dynamic_slot; 重启策略 (全部强制终止 active slot) 在 daemon bootstrap")

      (writer autopilot-ttl-gc
        :binds-to [:worker-trait-surface]
        :cross-ref "pillar 二 2.4 :: autopilot (daemon/src/engine/intent_engine/autopilot.rs)"
        :writes    "dynamic_slots (terminate on 'ttl_expired')"
        :library-pov "库暴露 find_expired_dynamic_slots 扫描接口 + terminate; tick 频率 + 决策在 autopilot"))

    (module-core
      (desc "3 张 slot 运行时表 — schema + 语义; 统一通过 SlotStore super-trait 访问")
      :trait "SlotStore (crates/missiond-core/src/db/traits.rs :: SlotStore trait body) — 注: 此 trait 还管 daemon_state + legacy tasks/inbox/events, 跨 slot-support / system-support / legacy 多模块"

      (plumbing slot-sessions
        (desc "slot ↔ PTY session 的 1:1 绑定表")
        :table "slot_sessions"
        :schema-cols "slot_id (PK) / session_id NOT NULL / updated_at BIGINT"
        :write-pattern "upsert on conflict (slot_id) DO UPDATE"
        :trait-methods "get_slot_session / set_slot_session / delete_slot_session / get_all_slot_sessions / get_slot_for_session / cleanup_pty_placeholder"
        :bidirectional "slot→session (get_slot_session) + session→slot (get_slot_for_session, 用于 reconcile)"
        :code "crates/missiond-core/src/db/slot.rs")

      (plumbing slot-tasks
        (desc "learning engine AI 任务队列 (extraction + decision 两类)")
        :table "slot_tasks"
        :schema-cols "id (PK) / slot_id / task_type / status (default 'pending') / prompt_summary / source_sessions / output_count / created_at / started_at / completed_at / duration_ms / error / conversation_id"
        :indexes "3 个: (slot_id) / (task_type) / (created_at)"
        :state-machine "pending → running → completed / failed"
        :trait-methods "insert_slot_task / slot_task_set_running / _set_completed / _set_failed / list_slot_tasks / slot_task_stats / reap_stale_slot_tasks / find_stale_decision_tasks / cleanup_orphan_slot_tasks / last_completed_slot_task_at / get_running_slot_task"
        :note "task_type 实际 values 由 learning engine 决定 (extraction / decision 是已知两类)"
        :code "crates/missiond-core/src/db/slot.rs-137")

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
        :binds-to [:mcp-surface]
        :tool "mission_slots"
        :reads "slot_sessions + dynamic_slots"
        :code "daemon/src/handlers/sysinfra/misc.rs (与 mission_slot_history / mission_inbox 共用 handler)"
        :purpose "列当前 slot + session 绑定 + active dynamic slots")

      (reader mcp-slot-history
        :binds-to [:mcp-surface]
        :tool "mission_slot_history"
        :reads "slot_tasks"
        :code "daemon/src/handlers/sysinfra/misc.rs (调 list_slot_tasks)"
        :purpose "slot 内 AI 任务历史 (含 pending / running / completed / failed)")

      (reader mcp-compute-slot-query
        :binds-to [:mcp-surface]
        :tool "mission_compute_slot (action=list)"
        :reads "dynamic_slots"
        :code "daemon/src/handlers/compute/compute_slot.rs (调 list_dynamic_slots)")

      (reader mission-memory-task-trail
        :binds-to [:mcp-surface]
        :tool "mission_memory (附加 slot task 快照)"
        :code "daemon/src/handlers/knowledge/memory.rs"
        :reads "slot_tasks (按 slot_id 拉最近 10 条)"
        :purpose "项目 memory 查询时附带 slot 执行轨迹")

      ;; ── daemon 内部 readers ──
      (reader daemon-startup-recovery
        :binds-to [:worker-trait-surface]
        :cross-ref "daemon/src/main.rs"
        :reads "list_dynamic_slots('active') + get_all_slot_sessions"
        :purpose "daemon 启动时: (1) 扫 active 动态 slot 做 terminate 清理 (2) 恢复现有 slot↔session 映射")

      (reader reconcile-workers
        :binds-to [:worker-trait-surface]
        :cross-ref "daemon/src/workers/local/reconcile_worker.rs + gemini_reconcile_worker.rs"
        :reads "get_slot_for_session (session → slot 反查)"
        :purpose "对账 workers 发现 session 时反查 slot 做归属判断")

      (reader events-sync-lookup
        :binds-to [:worker-trait-surface]
        :cross-ref "daemon/src/events_sync.rs"
        :reads "get_slot_for_session"
        :purpose "PTY 事件同步时 session → slot 反查")

      (reader ingestion-router-lookup
        :binds-to [:worker-trait-surface]
        :cross-ref "daemon/src/infra/ingestion_router.rs"
        :reads "get_slot_for_session"
        :purpose "JSONL 摄入路由时反查")

      (reader session-util
        :binds-to [:worker-trait-surface]
        :cross-ref "daemon/src/infra/session_util.rs"
        :reads "get_all_slot_sessions"
        :purpose "infra 工具读全部绑定"))

    (module-tables-owned
      (desc "本模块独占 3 张表")
      (tables slot_sessions slot_tasks dynamic_slots)
      (count 3)
      (migrated-from "v0.4.11 category system-support :: compute-runtime component"))

    (cross-module-trait-sharing
      (desc "SlotStore super-trait 横跨多模块, 本模块只用其中一部分")
      :shared-trait "SlotStore (db/traits.rs :: SlotStore trait body)"
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
        :benefit  "下轮 migration 可定向 DROP")
      (goal-4 capability-usage-observability
        :problem "tool / flow 会随更好实现出现而落灰; 单看 registry 无法判断保留、合并、废弃"
        :solution "派生 read-model 统一读取 tool_calls / event_log / flow_context / workflow stats,生成 capability usage snapshot 与治理候选"
        :benefit "持续优化工具面和 flow catalog,让删改合并有证据而不是凭感觉"))

    (derived-read-model capability-usage-read-model
      :status "code-aligned v0.5.6 — semantic evidence v1 已落 (commit 3ed14fc): 5 sources + lisp hint merge-candidate + replacement_target 字段; semantic merge 自动决策 / event_log 之外 source 聚合 / workflow stats 仍 pending"
      :owner-module "system-support"
      :storage-policy "当前实现按需聚合,不缓存 snapshot: daemon_state trait 只支持 i64,不能存 JSON; mark/ack/merge 治理决定写 <project_root>/.missiond/v2/capability-usage-review.json sidecar (含 replacement_target/ack_at/ack_by/follow_up_ref)"
      :implementation-targets ["crates/missiond-mcp/src/tools/comm/capability_usage.rs (schema + replacement_target)"
                               "crates/missiond-daemon/src/handlers/comm/capability_usage.rs (HintIndex 解析 .missiond/v2/intent-{tools,flow,intent-layer}.lisp + probe_event_log_flows + ReviewState sidecar + protected source/target rejection)"]
      :truth-sources
        ((tool-calls
           :status "implemented"
           :reads ["conversation_tool_calls via ConversationStore::get_tool_call_global_stats(since_iso)"]
           :grain "tool_name + action + project_id? + caller + outcome + started_at/completed_at"
           :purpose "统计每个 MCP tool/action 的调用次数、最近调用、成功率、调用来源")
         (flow-runs
           :status "implemented"
           :reads ["board_tasks.flow_template" "engine::flow::loader::list_flows() YAML registry (合并 generated + core)"]
           :grain "flow_id/flow_template + node/action + project_id? + terminal status"
           :purpose "统计 named flow 与 executable workflow 的启动、完成、失败、最近使用"
           :pending ["workflow execution stats / success rate" "flow_context deep evidence"])
         (event-log-flow-events
           :status "implemented (read-only probe)"
           :reads ["event_log via query_timeline_filtered(\"board::task_created\", since=90d, limit=500); 抽 payload.flow_template / .template / .task.flow_template"]
           :grain "flow_template + event count + first/last seen"
           :purpose "补充 board_tasks 之外的 flow 事件证据 (read-only, 不写)"
           :pending ["DispatcherEvent / WorkerEvent / ExecutionEvent 等其他 event source 聚合"])
         (lisp-semantic-hints
           :status "implemented"
           :reads [".missiond/v2/intent-tools.lisp" ".missiond/v2/intent-flow.lisp" ".missiond/v2/intent-intent-layer.lisp"]
           :purpose "扫描 :replacement \"mission_…\" / :moved-to \"F-…\" / :supersedes \"…\" / :status \"deprecated-…\" / consolidated-keep 标记, 形成 HintIndex"
           :merge-candidate-rule "仅在 hint token ∈ {deprecated, moved-to, replacement, supersedes} 且 target 解析为已注册 tool/flow 且 target ≠ source 且 target 非 protected 时给出; consolidated-keep 显式压制")
         (review-sidecar
           :status "implemented"
           :reads [".missiond/v2/capability-usage-review.json"]
           :purpose "读出 mark/ack/merge 历史决策 + replacement_target 关联"))
      (ingress
        :source "mission_capability_usage(action=snapshot|report|candidates|mark|ack); future periodic monitor tick"
        :entry-components ["tools audit log" "event-bus event_log (board::task_created)" "board/workflow state" ".missiond/v2/*.lisp hints" "intent-layer governance rules"])
      (logic-core
        (step s1 "collect tool usage by window: 7d / 30d / 90d / all-time from conversation_tool_calls")
        (step s2 "collect flow usage from board_tasks.flow_template + event_log probe + YAML flow registry (generated + core)")
        (step s3 "build HintIndex 扫 3 lisp 文件; normalize against MCP registry; protected source/target 在治理规则里")
        (step s4 "apply protected capability patterns from intent-layer governance")
        (step s5 "emit categories: active / quiet / stale / never-used / shadowed-by-better-capability / merge-candidate / protected (merge-candidate 仅 hint-driven)")
        (step s6 "return source_coverage.sources[5] with read-only persistence note; never mutate registry or Lisp directly")
        (step s7 "mark/ack/merge write review sidecar only; merge 必须 replacement_target 显式或 hint 解析得到, target protected 拒绝"))
      (egress
        :writes ["<project_root>/.missiond/v2/capability-usage-review.json for mark/ack/merge only" "ObservabilityEvent::CapabilityUsageSnapshot / CapabilityStaleCandidate"]
        :reads ["conversation_tool_calls" "board_tasks.flow_template" "event_log (board::task_created)" "MCP registry" "YAML flow registry (generated + core)" ".missiond/v2/intent-{tools,flow,intent-layer}.lisp hints" "review sidecar"]
        :returns "capability usage snapshot (含 source_coverage.sources[5]) + stale/merge/deprecate candidates with evidence")
      :pending ["semantic merge 自动决策 (即使 hint 完整, 也只产候选, 不动 registry)"
                "DispatcherEvent / WorkerEvent / ExecutionEvent 等其他 event source 聚合"
                "workflow execution stats / success rate"])

    (module-ingress
      (desc "观测 + infra + legacy 3 类写入")
      :principle "memory = 库. worker / handler / event-bus fanout 各管时机 + 业务"

      ;; ── 观测 writers (3) ──
      (writer aiops-incident-reactor
        :binds-to [:worker-trait-surface]
        :cross-ref "daemon/src/infra/aiops.rs process_incident (写入点 192 / 338)"
        :trigger "event-bus IncidentEvent::Reported subscriber (bus/v2_subscribers.rs v2_incident_reactor)"
        :writes "incidents (含 dedupe_key 去重)"
        :trait-methods "ObservabilityStore::insert_incident / has_recent_incident / list_incidents"
        :library-pov "库暴露 insert + dedup check; triage 逻辑在 aiops")

      (writer router-chat-archiver
        :binds-to [:worker-trait-surface]
        :cross-ref "daemon/src/handlers/comm/router_chat.rs + pg/observability.rs router_chat_clear 族"
        :trigger "router_chat_clear / _clear_by_task 等时机 — 归档被删除 / 压缩的旧消息"
        :writes "router_chat_archive (original_id / session_id / role / content / archive_reason)"
        :trait-methods "ObservabilityStore::router_chat_* (22 方法族)"
        :library-pov "库暴露 router_chat_* 方法族; archive 策略在 handler")

      (writer vision-worker
        :binds-to [:worker-trait-surface]
        :cross-ref "pillar 二 2.3 :: workers/codex/vision_worker.rs :: save_image_description"
        :writes "image_descriptions (upsert by image_hash)"
        :library-pov "库暴露按 hash upsert; vision LLM 调用 + 去重决策在 worker")

      ;; ── infra 游标 + backfill writers (7) ──
      (writer watcher-cursor-writers
        :binds-to [:worker-trait-surface]
        :cross-ref "多个 file-watcher 类 worker (具体 worker 待补 — 方法签名是 batch upsert)"
        :writes "watcher_cursors (file_path PK / byte_offset / session_id)"
        :trait-methods "ObservabilityStore::load_watcher_cursors / upsert_watcher_cursors_batch / delete_watcher_cursor"
        :library-pov "库暴露 load/batch-upsert/delete; watcher 算法在 worker")

      (writer reconcile-worker-watermarks
        :binds-to [:worker-trait-surface]
        :cross-ref "pillar 二 2.3 :: workers/local/reconcile_worker.rs"
        :writes "reconcile_watermarks (jsonl_path PK / last_reconciled_size / last_reconciled_at)"
        :trait-methods "ObservabilityStore::get_reconcile_watermark / upsert_reconcile_watermark / get_all_reconcile_watermarks")

      (writer gemini-reconcile-watermarks
        :binds-to [:worker-trait-surface]
        :cross-ref "pillar 二 2.3 :: workers/local/gemini_reconcile_worker.rs"
        :writes "gemini_cli_watermarks (session_file PK / session_id / last_msg_count)"
        :trait-methods "ObservabilityStore::load_gemini_cursors / save_gemini_cursor")

      (writer consumer-watermarks-writers
        :binds-to [:worker-trait-surface]
        :cross-ref "多 consumer 通过 watermark_* API 更新 (具体 consumers 散在多 worker)"
        :writes "consumer_watermarks (consumer_name + session_id 复合 PK / last_processed_msg_id / extra)"
        :trait-methods "ObservabilityStore::watermark_get / _advance_time / _advance_msg_id / _advance_full / _list")

      (writer embedding-worker-backfill
        :binds-to [:worker-trait-surface]
        :cross-ref "pillar 二 2.3 :: workers/sonnet/embedding_worker.rs (写入点 1236 / 1322 / 1327 / 1343 / 1544 / 1597 / 1663 / 1912 / 1936 / 1953)"
        :writes "backfill_progress (phase 进度) + backfill_failures (重试记录)"
        :known-phases "conv_topic_vectors / conv_summary (session-level embedding backfill)"
        :trait-methods "MissionDB inherent: backfill_start_phase / _update_progress / _complete_phase / _record_failure / _retryable_failures / _retryable_failures_no_cooldown / _clear_failure / _get_phase / _list_phases"
        :correction-v0.4.15 "原以为是一次性迁移表 — 实际是 embedding-worker 持续的 phased 作业状态管理, active")

      (writer daemon-state-writer
        :binds-to [:worker-trait-surface]
        :cross-ref "daemon bootstrap + 各 state 更新处 (多处)"
        :writes "daemon_state (key PK / value / updated_at)"
        :trait-methods "SlotStore::daemon_state_get / _set"
        :note "trait 归 SlotStore (db/slot.rs 同文件), 实际用途是 daemon 全局 KV")

      ;; ── Legacy zone writers (v0.5.0: tasks 已 DROP, 迁到 board_tasks) ──
      (writer-removed memory-hook-tasks-writers
        :removed-at "v0.5.0 (tasks 表迁到 board_tasks)"
        :was "memory_scheduler 的 memory hook 入口, 写 tasks 表"
        :now "改写 board_tasks with trigger_source='memory_hook', 详见 module board :: writer memory-hook-submitter")

      (writer strategy-worker-inbox
        :binds-to [:worker-trait-surface]
        :cross-ref "pillar 二 :: workers/gemini/strategy_worker.rs (仅此一处)"
        :writes "inbox (from_role + content, 给某 task 发消息)"
        :status "⚠ DEPRECATE — 轻度使用, 单 caller"
        :library-pov "库暴露 insert_inbox_message; 何时发由 strategy worker")

      (writer-removed events-writers
        :removed-at "v0.4.24 Phase 6 (migration 20260421000000)"
        :was "task.rs :: insert_event (legacy 路径)"
        :superseded-by "pillar 四 event_log SSOT")

      (writer-removed credentials-writers
        :removed-finding "v0.4.15 调查发现 — 零 Rust CRUD"
        :removed-at "v0.4.24 Phase 6 (migration 20260421000000)"
        :was "无实际 writer (仅 schema)"
        :note "墓志铭保留, 供知情"))

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
          :status "⚠ PARTIAL MIGRATION v0.5.0 — memory-hook 迁 board_tasks, tasks 表保留给 pillar 二 compute"
          :D010-correction "v0.5.0 设计 agent 调研发现 tasks 实际跨 pillar: 30+ caller 中 25 处在 pillar 二 (mission_task_submit MCP 族 + supervisor + pty + context_pipeline + extraction)"
          :ownership-revelation "tasks 表按代码使用实际归 pillar 二 compute, 不是 memory pillar. memory pillar 只管 memory-hook 子集"
          :v0.5-actions-done
            "✅ board_tasks ALTER TABLE ADD COLUMN trigger_source TEXT (28 列)"
            "✅ BoardStore::list_board_tasks_by_trigger 新增"
            "✅ state::submit_task 改写 board_tasks (trigger_source='memory_hook', role='memory')"
            "✅ memory_scheduler poll board_tasks WHERE trigger_source='memory_hook'"
            "✅ reap_stale → BoardStore::recover_stale_running_tasks"
            "✅ memory pillar 5 caller 迁移: state / kb / conversation_logger / pty_event_worker / board"
          :v0.5-actions-NOT-done
            "❌ DROP TABLE tasks (pillar 二 仍在用)"
            "❌ SlotStore 8 tasks 方法 (mission_task_submit MCP family 仍依赖)"
          :pillar-二-remaining-callers
            "handlers/compute/task.rs (mission_task_submit/_query/_cancel/_ack/_track MCP 族, 500+ 行)"
            "supervisor.rs (slot 死亡恢复)"
            "handlers/compute/pty.rs (PTY 重启 requeue)"
            "context/context_pipeline.rs (ack_completed_tasks)"
            "engine/learning_engine/extraction.rs (get_tasks_by_status)"
          :next-step "pillar 二 compute 整理时决定 — 可能: (a) 把 mission_task_submit 改写到 board_tasks; (b) tasks 保留作 compute 专用队列; (c) 其他策略"
          :cross-pillar-ref "按 'ownership by usage' 原则, tasks 按'使用方'应归 pillar 二; memory pillar 不再管. 类比 TimelineStore 归 pillar 四 的模式.")

        (table inbox
          :status "⚠ DEPRECATE — 轻度使用"
          :criticality "🟢 LOW — 仅 1 处 active writer"
          :caller-count 1
          :active-callers (workers/gemini/strategy_worker.rs "Gemini 策略 worker 发消息给某 task (insert_inbox_message)")
          :writers "crates/missiond-core/src/db/task.rs (insert_inbox_message / get_inbox_messages / mark_inbox_read)"
          :schema-cols "id (PK) / task_id / from_role / content / read / created_at"
          :indexes "(read), (created_at)"
          :trait "SlotStore trait (3 inbox 方法)"
          :mcp "mission_inbox (共用 misc handler, v0.4.24 仍保留)"
          :migration-cost "🟢 LOW — 单 caller, 迁到 board_task_notes 或直接删"
          :suggested-successor "board_task_notes (加 from_role 列)"
          :action "v0.4.25+ 迁移 strategy_worker 到 board_task_notes 后 drop")

        (table events
          :status "✅ DROPPED — Phase 6 执行 (migration 20260421000000_drop_deprecated_tables.sql)"
          :pre-drop-writers "仅 task.rs :: insert_event (legacy 路径, 无外部 active 消费)"
          :pre-drop-schema "id (PK auto) / task_id / type / data / timestamp"
          :superseded-by "pillar 四 event_log (SSOT)"
          :removed-at "v0.4.24 施工 Phase 6"
          :trait-method-removed "SlotStore::insert_event + get_events_by_task (2 方法, 连同 2 caller 清理)"
          :note "lisp v0.4.24 保留此条目作墓志铭 (tombstone), 供施工 agent 知历史")

        (table credentials
          :status "✅ DROPPED — Phase 6 执行 (migration 20260421000000_drop_deprecated_tables.sql)"
          :pre-drop-writers "无 Rust CRUD 代码"
          :pre-drop-readers "无"
          :pre-drop-schema "id (PK) / knowledge_id (FK → knowledge ON DELETE CASCADE) / name / value_encrypted / created_at / updated_at"
          :investigation "grep 全 crates/ 显示零 CRUD; 表创建但从未使用"
          :removed-at "v0.4.24 施工 Phase 6"
          :note "墓志铭保留, 供知情")))

    (module-egress
      (desc "MCP 读 + daemon 内部 consumer")

      ;; ── MCP readers (5) ──
      (reader mcp-incident
        :binds-to [:mcp-surface]
        :tool "mission_incident"
        :reads "incidents"
        :code "daemon/src/handlers/comm/question.rs (共用 handler, 与 mission_question / llm_trace 等)")

      (reader mcp-router-chat
        :binds-to [:mcp-surface]
        :tools "mission_router_chat / mission_router_chat_manage"
        :reads "router_chat_archive + conversations (chat_type='router_chat')"
        :code "daemon/src/handlers/comm/router_chat.rs")

      (reader mcp-sys-config-logs
        :binds-to [:mcp-surface]
        :tools "mission_sys_config / mission_sys_logs / mission_daemon_update"
        :reads "daemon_state + 各种 watermark 表 + backfill_progress"
        :code "daemon/src/handlers/system.rs")

      (reader mcp-infra-query
        :binds-to [:mcp-surface]
        :tools "mission_infra_query / mission_infra_ops"
        :reads "reconcile_watermarks / gemini_cli_watermarks / consumer_watermarks / watcher_cursors"
        :code "daemon/src/handlers/sysinfra/infra.rs")

      (reader mcp-inbox-legacy
        :binds-to [:mcp-surface]
        :tool "mission_inbox"
        :reads "inbox (legacy v1 API)"
        :code "daemon/src/handlers/sysinfra/misc.rs (共用 misc handler)")

      ;; ── daemon 内部 readers (4) ──
      (reader vision-enrichment
        :binds-to [:worker-trait-surface]
        :cross-ref "daemon 内消息 enrichment 流程"
        :reads "image_descriptions (按 image_hash 查)"
        :purpose "消息含图片时查找缓存的 LLM 描述")

      (reader backfill-resume
        :binds-to [:worker-trait-surface]
        :cross-ref "embedding_worker.rs 启动时恢复"
        :reads "backfill_progress (get_phase) + backfill_failures (retryable_failures)"
        :purpose "daemon 重启后从上次 phase 继续")

      (reader watermark-consumers-readers
        :binds-to [:worker-trait-surface]
        :cross-ref "多 consumer workers"
        :reads "consumer_watermarks / watcher_cursors / reconcile_watermarks / gemini_cli_watermarks"
        :purpose "每 worker 启动时读自己的游标做增量处理")

      (reader incident-enrichment
        :binds-to [:worker-trait-surface]
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
        ("ObservabilityStore (db/traits.rs :: ObservabilityStore trait body) — 大 trait, 本模块用其中的: incident 方法 (3) + watcher_cursors (3) + reconcile_watermarks (3) + gemini_cli_watermarks (2) + consumer_watermarks via watermark_* (5) + router_chat (22)"
         "SlotStore (db/traits.rs :: SlotStore trait body) — v0.5.0 后: daemon_state 已拆到 InfraStore (Stage 2D); events 2 方法已删 (Phase 6); tasks 8 方法保留给 pillar 二 mission_task_submit 族 (D010 发现); inbox 3 方法保留 (单 caller pillar 二 strategy_worker)")
      :inherent-methods "backfill.rs 9 方法是 MissionDB inherent 方法, 不在 trait"
      :other-modules-use-same-traits
        ("llm-support: ObservabilityStore 的 gemini_log + gemini_file_cache + token_usage + token_stats 方法"
         "conversation-logs: ObservabilityStore 的 label_* + conversations_missing_summary_* 方法"
         "slot-support: SlotStore 的 slot_sessions + slot_tasks + dynamic_slots 方法")
      :design-note "trait 按实现文件切分 (slot.rs + observability.rs + gemini_log.rs + incident.rs 等) — 同 llm-support / slot-support 的阻抗不匹配模式, 接受"))


  ;; ═════════════════════════════════════════════════════════════
  ;;  Support 模块 9: Embedding Support — embedding 列跨表治理
  ;;  v0.4.21 从 cross-cutting capability 升格为 module
  ;;  特殊: 0 张独占表, 管 "列契约 + policy" 而非 "表 row ownership"
  ;; ═════════════════════════════════════════════════════════════
  (module embedding-support
    (desc "embedding 列 schema + policy 治理层 — 跨 5 张承载表 + 1 张 audit 表的统一契约")
    (maturity "成熟 (v0.4.21 从 cross-cutting capability 升格)")
    :created "v0.4.21 (2026-04-20)"
    :migrated-from "v0.4.21 从 cross-cutting :: capability embedding-storage-governance 升格为 module"
    :rationale "embedding 治理横跨 3 business module 的 5 张承载表; 独立成 module 便于集中管 schema + policy + provider + retention + change-protocol"
    :special-nature "0 张独占表 — 管 '列契约 + policy', 不 own 整表. 承载表的 row-level ownership 仍归原 module (kb-manager / conversation-logs / project-management)"

    (primary-goals
      (goal-1 schema-uniformity
        :problem  "embedding 列分散在 5 张表, 维度/索引参数若不统一, 查询相似度不可比"
        :solution "集中声明 dimension=512 + HNSW 参数 + vector/halfvec 类型选择, 承载表必须遵守"
        :benefit  "同一 query 可跨表召回; HNSW 参数按表大小调优 (message_embeddings m=24 vs 其他 m=16)")
      (goal-2 provider-singularity
        :problem  "若允许多 provider, 向量空间不兼容 — 同一 query 向量不能跨 provider 搜"
        :solution "唯一 provider qwen3 via pillar 二 2.2 sonnet-gateway, 禁止降级兜底"
        :benefit  "向量空间单一, 相似度可比; 失败 fail-fast 暴露问题")
      (goal-3 skip-audit
        :problem  "embedding_worker 有多种跳过逻辑 (tool_result / system / empty / role), 需审计"
        :solution "message_embedding_skips audit 表 — 记录 message_id + skip_reason"
        :benefit  "可追溯跳过决策; 区别于 dedup 表 (dedup 在 EmbedDecision enum)")
      (goal-4 change-protocol
        :problem  "维度/provider 变更影响所有 5 张表 + 索引 + 消费者"
        :solution "明文 change-protocol: dim-change 迁 5 表 + 5 索引 + provider 切换; provider-change 重算全表"
        :benefit  "变更路径可审阅, 避免悄悄破坏相似度"))

    (module-ingress
      (desc "writer 不在本 module — embedding 生成在 pillar 二 worker, 本 module 只暴露 schema+policy 契约")
      :principle "memory = 库. 本 module 治理 '列' 的约束 + policy, 生成任务完全外包 pillar 二"

      (contract-with-generator
        :binds-to [:worker-trait-surface]
        :cross-ref "pillar 二 2.3 :: workers/sonnet/embedding_worker.rs"
        :invariant-1 "失败 fail-fast 不兜底 (见 embedding-worker :provider)"
        :invariant-2 "批量写入, 单次事务 (insert_message_embeddings_batch / set_conversation_topic_vectors / kb_remember)"
        :invariant-3 "provider 固定 qwen3, 换需重算全表"
        :writes-to-tables "承载表由各自 module 管写入接口; 本 module 只约束 '写入的数据形态 (512 dim vector / halfvec)'"
        :library-pov "embedding-support 暴露列 schema 约束; embedding 生成算法 / 调度 / provider 绑定在 worker"))

    (module-core
      (desc "5 plumbing: dimension 契约 + 5 承载表目录 + skip audit + retention + change protocol")

      (plumbing dimension-contract
        (desc "embedding 向量的维度契约 — 全系统唯一")
        :dimension 512
        :provider-binding "qwen3 via pillar 二 2.2 sonnet-gateway (唯一 provider, 禁止降级兜底)"
        :index-type "HNSW (pgvector extension, cosine similarity)")

      (plumbing storage-tables-catalog
        (desc "5 张承载 embedding_vec 列的表 — 按 column ownership (本 module) vs row ownership (原 module) 双轨")
        (knowledge
          :column "embedding_vec"
          :type "vector (standard)"
          :hnsw-params "m=16, ef_construction=64"
          :owned-by-rows "module kb-manager"
          :owned-by-column "module embedding-support")
        (conversation_topic_vectors
          :column "embedding_vec"
          :type "vector"
          :hnsw-params "m=16, ef_construction=64"
          :owned-by-rows "module conversation-logs"
          :owned-by-column "module embedding-support")
        (message_embeddings
          :column "embedding_vec"
          :type "halfvec (1KB 压缩)"
          :hnsw-params "m=24, ef_construction=128"
          :rationale "消息量大, halfvec 节省空间 + 更深索引"
          :owned-by-rows "module conversation-logs"
          :owned-by-column "module embedding-support")
        (skill_topics
          :column "embedding_vec"
          :type "vector"
          :hnsw-params "m=16, ef_construction=64"
          :owned-by-rows "module project-management"
          :owned-by-column "module embedding-support")
        (ast_nodes
          :column "embedding_vec"
          :type "vector"
          :hnsw-params "m=16, ef_construction=64"
          :owned-by-rows "module kb-manager"
          :owned-by-column "module embedding-support"))

      (plumbing skip-audit-policy
        (desc "跳过 embed 决策的 audit trail")
        :table "message_embedding_skips"
        :owned-by-rows "module conversation-logs (表在那儿登记)"
        :owned-by-policy "module embedding-support (policy + writer 约束在本 module)"
        :schema "message_id (PK, FK→conversation_messages.id) + skip_reason TEXT"
        :writer "embedding_worker.rs :: insert_message_embedding_skips_batch (backfill 分桶时批量写)"
        :decision-source "embedding_worker.rs :: should_embed() 返 EmbedDecision enum"
        :readers "db/pg/conversation.rs 的统计查询 (COUNT + GROUP BY skip_reason)"
        :note "⚠ 重要: 这是 audit log 不是 dedup 表. 实际去重在 EmbedDecision enum pattern 做, skip 表只留痕"
        :reasons-examples "tool_result / system message / empty / role-based skip")

      (plumbing retention-strategy
        (desc "embedding 列 retention 策略")
        :strategy "follow-parent"
        :reason "embedding 是 parent row 的派生物, 无独立 TTL"
        :note "parent row 删除 → embedding 随之失效 (CASCADE 或应用层清理)")

      (plumbing change-protocol
        (desc "改 embedding 契约的影响面")
        :dim-change "改 512 → 1024: 需同时迁移 5 承载表 + 5 HNSW 索引 + provider 切换"
        :provider-change "换 qwen3 → 别的: 需重算所有 5 表 embedding; 中间态不可搜, 建议离线 backfill"
        :new-table-adding-embedding "(1) 迁移加 embedding_vec 列 + HNSW 索引 (2) 本 module storage-tables-catalog 补记 (3) embedding-worker 加写入路径"))

    (module-egress
      (desc "reader 不在本 module — embedding 消费在 pillar 二 search-engines, 本 module 只暴露 query 契约")

      (contract-with-consumer
        :binds-to [:worker-trait-surface]
        :cross-ref "pillar 二 2.6 search-engines :: engine vector-hnsw"
        :invariant-1 "consumer 必须容忍 NULL embedding (async 生成未就绪或跳过)"
        :invariant-2 "查询时用同 provider 生成 query 向量, 否则相似度不可比"
        :fusion-with "FTS + trigram + tag (见 pillar 二 2.6 fusion-ranker)"
        :library-pov "embedding-support 暴露 '列存在 + 类型 + HNSW 索引' 契约; 查询策略 / RRF 融合 / 相关性打分在 search-engines"))

    (module-tables-owned
      (desc "0 张独占表 — 本 module 管 '列契约 + policy', 不 own 整表")
      (tables)
      (count 0)
      (special-ownership-model "column-ownership vs row-ownership 双轨: 承载表的行归原 module (kb-manager / conversation-logs / project-management), embedding 列的 schema + policy 归本 module")
      (tables-managed-by-column
        (knowledge                  :column-only "embedding_vec" :rows-owned-by "module kb-manager")
        (conversation_topic_vectors :column-only "embedding_vec" :rows-owned-by "module conversation-logs")
        (message_embeddings         :column-only "embedding_vec" :rows-owned-by "module conversation-logs")
        (skill_topics               :column-only "embedding_vec" :rows-owned-by "module project-management")
        (ast_nodes                  :column-only "embedding_vec" :rows-owned-by "module kb-manager")
        (message_embedding_skips    :audit-policy-only "full table"  :rows-owned-by "module conversation-logs"))
      (non-db-forms-owned
        (embedding-vectors-form "form 在 non-db-forms, v0.4.21 起归本 module (原只 cross-ref 到 cross-cutting capability)")))

    (cross-module-trait-sharing
      (desc "本 module 不定义新 trait — 承载表的 trait 仍归原 module, 本 module 只约束列类型")
      :note "worker 通过 KbStore (knowledge / ast_nodes embedding) + ConversationStore (message / conv_topic / skips) + ProjectStore (skill_topics) 的方法写 embedding; 每个方法签名的 vec 类型由本 module 约束 (512 dim, vector/halfvec)"
      :known-issue-v0.4.6 "skill_topics embedding 归 project-management 的行 (因 skills 是 project-management 管); 但 search-engines 查 skill 时仍通过 vector-hnsw 统一路径"
      :known-issue-ast "ast_nodes embedding 归 kb-manager 的行 (kb-ast-linkage + code-search 消费)"))


  ;; ═════════════════════════════════════════════════════════════
  ;;  横切能力 (Cross-Cutting)
  ;; ═════════════════════════════════════════════════════════════
  (cross-cutting
    (desc "贯穿 5 business + 4 support module 的基础能力 (v0.4.21: embedding 治理升格为 module)")

    (capability db-trait-abstraction
      (desc "MissionStore 超 trait 聚合 9 个领域 store (见 pillar-interfaces worker-trait-surface)")
      :trait "MissionStore"
      :code "crates/missiond-core/src/db/traits.rs (~750 行)"
      :stores 9
      :stores-list "ProjectStore / BoardStore / KbStore / ConversationStore / MessageStore / ObservabilityStore / DirectiveLayerStore (code-aligned partial) / SlotStore / InfraStore"
      :v0.4.20-correction "原 lisp 标 13, 和 pillar-interfaces current-traits 9 不一致; 以 pillar-interfaces 为准"
      :invariant "其他 crate 只依赖 trait 不依赖实现"
      (impl pg-store :target "crates/missiond-core/src/db/pg_*/" :status production)
      (impl sqlite-store :status deprecated))

    (capability retention-policy
      (desc "按表粒度的保留/清理规则")
      :rules ("event_log: 30 天常规 / 3 天 ephemeral (pillar 四 SSOT) — 取代原 system_timeline 7 天"
              "gemini_requests: 7 天 (gemini-logger 启动 cleanup, 见 module llm-support)"
              "conversation_messages/events: append-only 无清理"
              "knowledge: append-only, access_count 手动归档"
              "incidents: 粗粒度按时间 DELETE"
              "tool_calls: append-only"
              "embedding 列: follow-parent (parent row 删则 embedding 随之, 见 embedding-storage-governance)")
      :default "其他未列表默认 append-only"
      :uncovered-candidates "token_usage_ledger / board_tasks (done 状态清理) / slot_tasks (completed 状态清理) / backfill_failures (成功后清) — 策略待定"
      :code "daemon/src/bus/retention_cron.rs + event/lifecycle/retention.rs (pillar 四)")

    (capability migrations-runner
      (desc "schema 演进 — daemon 启动 phase 1 自动跑")
      :code "crates/missiond-core/migrations/"
      :count 20
      :automation "sqlx::migrate! 编译期检查 + 运行期执行")

    (capability embedding-storage-governance
      :moved-to "memory :: module embedding-support (v0.4.21)"
      :reason "embedding 治理横跨 3 business module 的 5 张承载表 + 1 张 audit 表, 从 capability 升格为独立 module 管 schema+policy+provider+retention+change-protocol 契约"
      :see "module embedding-support :: module-core (dimension-contract / storage-tables-catalog / skip-audit-policy / retention-strategy / change-protocol)"))


  ;; ═════════════════════════════════════════════════════════════
  ;;  跨分类注记 — 演进路径 + 开放问题
  ;; ═════════════════════════════════════════════════════════════
  (cross-cutting-notes

    (migration-log
      :see "intent-memory-history.lisp :: memory-history :: (v0.4 ... v0.5.0 letter-numbered decisions A-DDDD)"
      :note "~290 行完整 migration log 外移, 减主 lisp context (L1 polish)")


    (ownership-summary
      ;; 5 business modules
      (module-project-management   5 "projects + 4 skills")
      (module-board                4 "board_tasks + board_task_notes + agent_questions + prompt_snapshots")
      (module-kb-manager           9 "knowledge + 4 kb_* + 4 ast/beacon")
      (module-conversation-logs   15 "conversations + 10 conv/message 派生 + retrospective_results + user_intents")
      (module-directive-layer      3 "directive + plan + workflow (store+manager code-aligned partial; actors pending)")
      ;; 3 support modules (v0.4.13/14/15 从 category system-support 分化)
      (module-llm-support          3 "gemini_requests + gemini_file_uploads + token_usage_ledger")
      (module-slot-support         3 "slot_sessions + slot_tasks + dynamic_slots")
      (module-system-support      14 "10 active (3 观测 + 7 infra) + 4 legacy (tasks/inbox/events/credentials)")
      (module-embedding-support    0 "v0.4.21: 从 cross-cutting capability 升格; 0 张独占表, 管 5 承载表 + 1 audit 表的列契约")
      ;; 外部
      (pillar-four-event-bus       4 "event_log (SSOT for timeline v1.3.0+) + event_subscriptions + blob_storage + dlq (not memory)")
      (pillar-five-intent-layer    0 "v0.4.17: 3 张表剥离到 memory directive-layer; pillar 五 仅保留非 DB 组件")
      (total 60)
      (memory-pillar-subtotal 56 "memory 管 5+4+9+15+3+3+3+14+0 = 56 张 (v0.4.21: +1 module embedding-support 但 0 独占表)")
      (delta-history "v0.4.4: -4 specs 迁 pillar 五; v0.4.16: +1 user_intents 校正回归; v0.4.17: +3 directive-layer 从 pillar 五 拿回; v0.4.21: +0 (embedding-support 是 column-only); 净 0"))

    (pending-actions
      "A. 给 candidates-for-promotion 中的高价值表加 project_id (token_usage_ledger / prompt_snapshots / directive+plan+workflow / skills 4 表)"
      "B. 确认 legacy tables (tasks / inbox / events / credentials) 是否可 drop + 写 migration"
      "[已解决 v0.4.14] ~~C. compute-runtime 归属~~ — slot-support module 建立后归属明确"
      "[已撤回 v0.4.19] ~~D. spec-db-sync~~ — 方向错 (forge 是单向服务方, 不是协作者); 且命名歧义已在 v0.4.19 rename 解决"
      "E. 派 agent 验证每个模块 owned-tables 在实际代码中是否被正确隔离 (不被其他 pillar 越界读写)"
      "F. directive-layer 剩余实现: directive-compiler / plan-compiler / workflow-distiller actors + project_id scoping; trait/reader/MCP manager 已完成 partial code-alignment"
      "G. drop 老的 narration 系列表 migration (message_narrations / narration_cursors, v0.4.12 声明但 migration 未写)")

    (design-rationale
      "v0.4 核心变化: 让每个业务模块承担自己表的所有权声明;"
      "不再把表都堆到 system-support; "
      "ownership 从隐含 (通过 :code 字段推断) 变成显式 ((module-tables-owned ...) 块)."
      "这使得其他 pillar / module 一眼能看出'这张表归谁管', 减少越界读写可能."))

) ;; end intent memory
