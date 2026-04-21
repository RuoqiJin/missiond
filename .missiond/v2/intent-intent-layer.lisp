;; ═════════════════════════════════════════════════════════════
;; MissionD — Intent-Layer Pillar (phase-A first-draft v0.1)
;; 目标: 元层 — 系统如何描述自己 / 被指挥 / 演化 / 认知推理
;; 底稿: gptpro intent-intent-layer.lisp (159 行 starter) + v2/intent.lisp 占位
;;       + worker v0.3 标的 intent-layer ownership 迁移项
;; 大原则: prescription / 认知 / 元层 归本 pillar; facts / runtime 归 memory + worker
;; ═════════════════════════════════════════════════════════════

(pillar intent-layer
  :version "v0.1"
  :status "phase-A first-draft 2026-04-21 — 本会话主驾"
  :predecessor "drafts/gptpro/intent-intent-layer.lisp (159 行 starter)"
  :target-path ".missiond/v2/intent-intent-layer.lisp"

  :actual-state-sources
    [".missiond/v2/intent.lisp :: pillar intent-layer (v0.4.17+ 已有详细占位)"
     ".missiond/v2/intent-worker.lisp v0.3 (DC014 决策: 7 sub learning-engine + flow-engine v1 迁入本 pillar)"
     ".missiond/v2/intent-memory.lisp v0.5.1 frozen (module directive-layer schema)"
     ".missiond/intent-pillar-engines.lisp :: learning-engine + flow-engine v1 (ground truth)"
     ".missiond/intent-pillar-state-machines.lisp :: engineering-phase + extraction-phase FSM"
     ".missiond/intent-pillar-event-workers.lisp :: lisp-survey-worker flow"
     ".missiond/intent-mcp-defs.lisp :: mission_intent + mission_forge_* schema"
     ".missiond/workflows/*.lisp (methodology 库 — pillar-refactor.lisp 等)"
     "$MISSIOND_HOME/flows/*.yaml (executable flow library)"]

  :design-correction-sources
    ["drafts/gptpro/intent-intent-layer.lisp (继承 5 component / 4 egress 大框架)"
     "worker v0.3 DC014 + dual-ownership 标注 (lisp_survey / arch_maintenance)"
     "intent-memory.lisp v0.5.1 :: module directive-layer (schema owned by memory, writer 归本 pillar)"]

  :historical-footprint-sources
    ["旧 learning-engine 曾全部归 worker pillar engine-cluster — v0.3 拆分后本 pillar 接手认知推理逻辑, worker 留 BackgroundWorker 触发"
     "旧 flow-engine v1 (flow_engine.rs) 与 v2 并存 — v1 是 project lifecycle phases (本 pillar), v2 是 YAML declarative (worker pillar)"
     "原 intent 表已 rename 为 directive 表 (v0.4.19), schema 归 memory :: directive-layer, writer 归本 pillar"]

  ;; ══════════════════════════════════════════════════════════
  ;; phase-A-decisions — 边界与迁移
  ;; ══════════════════════════════════════════════════════════
  (phase-A-decisions
    (Q-IL1
      :question "本 pillar 与 memory pillar 的边界?"
      :decision "memory = 库 (facts + schema + trait); intent-layer = prescription + 认知推理 + 元描述"
      :examples
        ["memory: user_intents 表 schema + ConversationStore trait"
         "intent-layer: intent_analyst 分析逻辑 (writer)"
         "memory: directive/plan/workflow 3 表 schema"
         "intent-layer: directive-plan-workflow 编译 pipeline 逻辑 (writer TBD)"])

    (Q-IL2
      :question "learning-engine 7 sub 是否全搬过来?"
      :decision "accept — 全搬. 在本 pillar 组织为 3 subsection (decision / extraction / analysis). 触发机制仍在 worker pillar"
      :related-worker-v0.3 "DC014 boundary-shift"
      :rationale "7 sub 都是'感知+推理+模式归纳'语义, 非 timer/dispatch mechanics")

    (Q-IL3
      :question "flow-engine v1 (flow_engine.rs) 归属?"
      :decision "accept — 搬本 pillar. v1 是 project lifecycle phases (Investigate → Plan → Execute 认知推进), 与 methodology workflows 同属元层"
      :distinguish "flow-engine v2 (YAML declarative runtime) 仍归 worker pillar"
      :state-machine-crossref "engineering-phase FSM 归本 pillar")

    (Q-IL4
      :question "双重归属 worker 如何标?"
      :decision "worker pillar = 触发 (event subscribe + BackgroundWorker impl); 本 pillar = 语义 ownership + 执行 prompt 内容"
      :dual-owned ["lisp_survey_worker (worker v0.3 已标)"
                   "arch_maintenance_worker (worker v0.3 已标)"]
      :cross-ref "每 dual-owned worker 在本 pillar 有独立 section, 详述 prompt/ slot / 产物 ownership; worker pillar 只描述触发点")

    (Q-IL5
      :question "directive-layer 3 表 (directive/plan/workflow) 当前 writer 是谁?"
      :decision "schema-ready-pending-implementation — 当前无 writer, 未来 3 种候选: (a) 本 pillar actor (新建) / (b) worker pillar 某 BackgroundWorker / (c) MCP 工具直写"
      :status "未来最可能 (a) — 创建独立 directive-compiler-actor")

    (Q-IL6
      :question "global CLAUDE.md manager 当前 vs 未来?"
      :decision "当前: Claude Code 自行读 ~/.claude/CLAUDE.md 作 system prompt, 无 daemon 侧 manager"
      :future "补 mission_global_instruction MCP tool (read/edit/reload)")

    (Q-IL7
      :question "workflows methodology lisp 与 executable YAML 的关系?"
      :decision "两种 kind 保留分离 (人类看 vs 机器跑), 未来可能 forge 冲压 (lisp 作 SSOT → yaml 副本), 当前不做"
      :cross-ref "memory pillar :: project-management 曾有 workflow-lisp-templates + flow-yaml-templates 两 component, v0.4.5 合并为单 workflows component 分 2 kind")

    (Q-IL8
      :question "intent-layer 是否调用 tools / worker / memory?"
      :decision "本 pillar 是 prescription source, 主向下派发 (→ tools 通过 MCP, → worker 通过 BackgroundWorker 触发, → memory 通过 trait writer)"
      :anti-pattern "本 pillar 不应被 worker / memory 调用 (反向依赖)"
      :exception "worker::lisp_survey_worker 触发 本 pillar::lisp-survey-update 是双向 — 但只是 trigger, 不是数据反流"))

  (purpose "元层 — 系统自我描述 + 指挥规约 + 认知推理 + 代码演化的唯一 ownership 源")

  (pillar-ingress
    (entry-1
      :source "人类 / 指挥官 编写或修改的 lisp / markdown / yaml 规约文件"
      :examples [".missiond/v2/*.lisp"
                 ".missiond/workflows/*.lisp (methodology)"
                 "$MISSIOND_HOME/flows/*.yaml (executable)"
                 "~/.claude/CLAUDE.md (global)"
                 "<project>/CLAUDE.md (project)"
                 "<project>/.missiond/intent.lisp (code snapshot)"])

    (entry-2
      :source "ContextualCommitDetected 事件 (worker 侧触发, 本 pillar 消费)"
      :consumers ["lisp-survey 更新 project intent.lisp"
                  "arch-maintenance 更新 architecture manifest"])

    (entry-3
      :source "MCP 工具调用"
      :tools ["mission_intent (read/section/summary/list)"
              "mission_forge_build / mission_forge_lint"
              "mission_project (init/context)"
              "future: mission_directive / mission_plan / mission_workflow / mission_execution"])

    (entry-4
      :source "认知推理 sub-engines 的自发触发"
      :mechanisms ["extraction pipeline on new conversation"
                   "decision cascade on agent question"
                   "idle-explore on system idle"
                   "historical-scan on backlog catch-up"])

    (entry-5
      :source "指挥官通过 Claude Code 会话启动自动加载 ~/.claude/CLAUDE.md"
      :mechanism "Claude Code 系统启动读取全局 CLAUDE.md 作 system prompt — 当前绕过 daemon"))

  (pillar-core
    (core-1 "intent files = 系统自我描述语料 (27+ lisp 文件生态)")
    (core-2 "forge = Lisp → IR → Rust 冲压器 (Generation Gap 隔离, 外部仓 ~/Projects/jarvis-forge)")
    (core-3 "governance = 代码模式声明 + lint + 文件级约束")
    (core-4 "action-instruction specs = directive / plan / workflow 三段 prescription 链 (schema 归 memory, writer 归本 pillar)")
    (core-5 "learning-engine 7 sub = 认知/学习/分析核心 (decision / extraction / analysis)")
    (core-6 "flow-engine v1 = project lifecycle phases 推进 (与 methodology workflows 同属元层)")
    (core-7 "lisp-survey + arch-maintenance = 项目代码变化 → intent 文件演化感知器 (dual-owned with worker)")
    (core-8 "workflows 双 kind: methodology-lisp (人类方法论) + executable-yaml (机器流水线)")
    (core-9 "prescription 原则: 本 pillar 管'系统应该如何', memory 管'系统现在记得什么', worker 管'系统在跑什么'"))

  (pillar-egress
    (egress-1 "→ memory pillar: 写 directive-layer 3 表 (schema 归 memory, writer 归本) + user_intents + conversation_turns.intent_group_id (via intent_analyst)")
    (egress-2 "→ worker pillar: 通过 event-bus 驱动 lisp_survey / arch_maintenance 触发")
    (egress-3 "→ tools pillar: 提供 mission_intent / mission_forge_* 后端逻辑")
    (egress-4 "→ flow pillar: workflows 定义 + engineering-phase FSM + methodology lisp")
    (egress-5 "→ 文件系统: <project>/.missiond/intent.lisp 更新 / <project>/CLAUDE.md managed sections / arch manifest / forge 输出代码")
    (egress-6 "→ event-bus: 发射 CascadeTriggered / CascadeCompleted / 认知分析完成事件")

    (cross-pillar-notes
      (memory
        :principle "prescription 写方 on 本 pillar, schema + trait on memory"
        :writers-owned-here
          ["intent_analyst → ConversationStore::insert_user_intent"
           "future directive-compiler → DirectiveLayerStore (pg/directive.rs 17 方法已存在)"]
        :tables-prescription-owned
          ["directive (schema-ready-pending-implementation)"
           "plan (schema-ready-pending-implementation)"
           "workflow (schema-ready-pending-implementation)"]
        :tables-cognitive-produced
          ["user_intents" "conversation_turns.intent_group_id"])

      (worker-migration-in
        :principle "worker v0.3 DC014 决策: runtime-mechanics 留 worker, 认知/学习归本 pillar"
        :migrated-ownership
          [(learning-engine-7-sub "decision_engine / decision_harvest / extraction / historical_scanner / idle_explorer / intent_analyst / timeline_analyst — 代码文件仍在 crates/missiond-daemon/src/engine/learning_engine/, 但语义 primary-ownership 在此 pillar")
           (flow-engine-v1 "flow_engine.rs 的 board-phase-engine 推进逻辑")
           (dual-owned-triggers "lisp_survey_worker + arch_maintenance_worker — 触发在 worker, prompt/slot/产物语义在此")]
        :state-machines-owned-here
          [(engineering-phase "Investigate → Consult → Plan → Execute → Finalize, 老图 intent-pillar-state-machines.lisp")
           (extraction-phase "Idle → Sending → WaitingForIdleness → Complete, 老图同")])

      (tools
        :surface "mission_intent / mission_forge_build / mission_forge_lint / mission_project + 未来 mission_directive / mission_plan / mission_workflow"
        :dispatch-principle "tools schema 归 tools pillar, handler 逻辑归本 pillar")

      (flow
        :relationship "workflows (methodology + executable) 的 SSOT 在此, 消费/执行在 flow pillar 与 worker::flow-engine-v2")

      (system-layer
        :not-direct "本 pillar 不直接穿越 system-layer, 但 forge 外部 CLI 算 system process")))

  ;; ══════════════════════════════════════════════════════════
  ;; 5.1 Self-Description Files — 自描述 lisp 文件生态
  ;; ══════════════════════════════════════════════════════════
  (section self-description-files
    :desc "系统自我描述的 lisp 文件总体 — 27+ file 生态 + 跨文件 graph 治理"
    :granularities ["L1-Blueprint (.missiond/v2/intent.lisp 总纲)"
                    "L2-Topology (.missiond/v2/intent-*.lisp 各 pillar)"
                    "L3-Implementation (.missiond/intent-db-*.lisp Forge 源 + .missiond/intent-pillar-*.lisp v1 地图)"]

    (component intent-files
      :desc ".missiond/*.lisp 意图声明, 按主题拆分并行加载"
      :count "27 v1 files + 3 frozen v2 (memory/event-bus/worker) + 1 v2 总纲 + 3 草稿 v0.1 (本 pillar/tools/flow)"
      :file-ownership
        [("L1 总纲 .missiond/v2/intent.lisp" :owner "本 pillar 主架构视图")
         ("L2 frozen v2 pillar 文件" :owner "各自 pillar")
         ("L3 旧地图 .missiond/intent-pillar-*.lisp" :owner "历史快照, 权威性随 v2 pillar 产出递减")
         ("Forge 源 .missiond/intent-db-*.lisp" :owner "Forge 冲压输入, schema SSOT")])

    (component intent-graph
      :desc "文件间 module-link / cross-ref 有向图, 供可视化与治理"
      :target "forge-daemon/src/intent_graph.rs (external repo ~/Projects/jarvis-forge)")

    (path system-intent-read
      :lifecycle-style on-demand
      (ingress
        :source "mission_intent(read/section/summary/list) / 人工打开 .missiond/v2/*.lisp"
        :entry-components
          ["crates/missiond-daemon/src/handlers/knowledge/intent.rs (handler)"
           "本 pillar 的 intent-files 生态 (文件 source)"])
      (logic-core
        (step s1 "按 project 或系统域定位目标 lisp 文件 (候选: .missiond/intent.lisp / .jarvis/intent.lisp / intent.lisp)")
        (step s2 "fuzzy project lookup (substring match, 单个自解 / 多个 'ambiguous' 返回候选)")
        (step s3 "按 action 分支: read(全文) / section(切 s-expression block) / summary(提 survey-date) / list(返回 {project_id, intent_path, exists}[])")
        (step s4 "保留 file path 与 survey-date 可追溯信息")
        (step s5 "返回 raw lisp / block / summary / file list"))
      (egress
        :writes []
        :reads []
        :file-reads ["<project>/.missiond/intent.lisp" "~/.claude/CLAUDE.md" ".missiond/v2/*.lisp"]
        :memory-cross-ref ["project-management (ProjectRegistry::resolve)"]
        :returns "intent raw content / section / summary / file list"
        :tools-surface "mission_intent (knowledge domain)"))

    (path intent-graph-build
      :lifecycle-style "on-demand / future"
      (ingress
        :source "lisp 文件集合变化 / 可视化治理请求"
        :entry-components ["forge-daemon/src/intent_graph.rs"])
      (logic-core
        (step s1 "扫文件间 module-link / cross-ref / :target / :binds-to")
        (step s2 "构建有向图")
        (step s3 "标出 orphan / cycle / broken-link / duplicate-target")
        (step s4 "供 governance 或可视化消费")
        (step s5 "输出 graph summary + diagnostics"))
      (egress
        :writes []
        :reads []
        :returns "intent-graph / governance diagnostics"
        :status "mostly TBD — forge-daemon 代码实际实现待确认")))

  ;; ══════════════════════════════════════════════════════════
  ;; 5.2 Forge Compilation — Lisp → IR → Rust 冲压器
  ;; ══════════════════════════════════════════════════════════
  (section forge-compilation
    :desc "外部冲压器 + governance lint — 把 Lisp 意图转成确定性 Rust 代码"
    :location "外部仓 ~/Projects/jarvis-forge (独立 git repo)"
    :breaks-if ["codegen-pattern-change" "ir-whitelist-change"]

    (component forge
      :desc "Lisp → IR → Rust Generation Gap 隔离冲压器"
      :target "~/Projects/jarvis-forge (外部独立仓)"
      :forge-binary "FORGE_BIN env override, default 'forge' CLI"
      :modes ["strict-codegen" "descriptive" "experimental"])

    (component governance
      :desc "Forge 模式声明 + 命名漂移检查 + broken-target 探测 + 一致性 cross-ref"
      :target "~/Projects/jarvis-forge/forge-daemon/src/governance.rs")

    (path forge-build
      :lifecycle-style on-demand
      (ingress
        :source "mission_forge_build MCP + build-time forge CLI"
        :entry-components
          ["crates/missiond-daemon/src/handlers/compute/forge.rs (missiond 侧 shell bridge)"
           "external jarvis-forge CLI"])
      (logic-core
        (step s1 "ProjectRegistry 查 project_id → 项目根目录")
        (step s2 "读系统或项目 intent lisp 作输入")
        (step s3 "按 governance 模式校验 (strict-codegen / descriptive / experimental)")
        (step s4 "Lisp → IR (外部 forge 进程)")
        (step s5 "IR → Rust 代码输出 (Generation Gap: generated.rs + custom.rs)"))
      (egress
        :writes []
        :reads ["projects"]
        :file-writes ["crates/*/src/**/generated.rs (Forge 输出)"]
        :memory-cross-ref ["project-management"]
        :returns "build result / exit_code / stdout / stderr"
        :tools-surface "mission_forge_build (compute domain)"
        :worker-bridge-crossref "worker pillar :: section worker-side-computation :: path forge-build-bridge")

    (path forge-lint
      :lifecycle-style on-demand
      (ingress
        :source "mission_forge_lint MCP + lisp_survey_worker (post-survey)"
        :entry-components
          ["crates/missiond-daemon/src/handlers/compute/forge.rs"
           "external jarvis-forge CLI (lint command)"])
      (logic-core
        (step s1 "按 project 定位项目根 + intent.lisp")
        (step s2 "执行 governance lint")
        (step s3 "检查模式违规、命名漂移、broken targets、不一致 cross-ref")
        (step s4 "输出 violations_raw + 建议修复项")
        (step s5 "阻止 build 或提示人工批准 (strict 模式)"))
      (egress
        :writes []
        :reads ["projects"]
        :returns "lint report / violations_raw / governance recommendation"
        :tools-surface "mission_forge_lint (compute domain)")))

  ;; ══════════════════════════════════════════════════════════
  ;; 5.3 Global CLAUDE.md — 全域总纲指挥层
  ;; ══════════════════════════════════════════════════════════
  (section global-claudemd
    :desc "全域总纲 — 指挥官对 Claude 的跨项目永久指令"
    :path "~/.claude/CLAUDE.md"
    :scope "global-user"
    :format "Markdown + 可选 YAML frontmatter"

    (component global-claudemd-file
      :desc "文件本身 — 全局偏好 / 行为约束 / 宇宙总纲"
      :path "~/.claude/CLAUDE.md"
      :loaded-by "Claude Code 系统启动时自动加载进 system prompt (当前绕 daemon)"
      :writer "用户手动编辑 / Claude Code Edit tool"
      :nature "元层约束 — 非业务记忆 (项目级约束见 memory :: project-management :: helper project-claudemd-manager)"
      :rationale "放本 pillar 而非 memory: 此文件是'系统如何被指挥'的声明, 属元层")

    (component global-claudemd-manager
      :desc "daemon 侧全局 CLAUDE.md 读/写/reload 管理器"
      :status "TBD — 当前无 daemon 侧 manager"
      :future-actions ["read" "edit" "reload"]
      :future-mcp-tool "mission_global_instruction (read/edit/reload)"
      :readers "Claude Code 每次会话启动 (当前绕 daemon)"
      :writers "用户手动 / Claude Code Edit tool (文件层)"
      :cross-ref "项目级 <project>/CLAUDE.md manager 在 memory :: project-management :: helper project-claudemd-manager (已实现)")

    (path global-claudemd-load
      :lifecycle-style "session-bootstrap"
      (ingress
        :source "Claude Code 会话启动 (每会话一次)"
        :entry-components ["~/.claude/CLAUDE.md (file)"
                           "Claude Code 系统 prompt loader (外部)"])
      (logic-core
        (step s1 "Claude Code 读取 ~/.claude/CLAUDE.md 全文")
        (step s2 "解析 Markdown + 可选 frontmatter")
        (step s3 "把全局偏好、约束、宇宙总纲注入 system prompt")
        (step s4 "与项目级 CLAUDE.md / intent / memory 形成分层 (全局 < 项目 < 会话)")
        (step s5 "未来 manager: 支持 read/edit/reload daemon 侧动作"))
      (egress
        :writes []
        :reads []
        :file-reads ["~/.claude/CLAUDE.md"]
        :returns "global instruction context"
        :status "partial — 当前 Claude Code 自行处理, daemon 侧 manager 待实现")

    (claude-md-sync-crossref
      :note "项目级 CLAUDE.md managed sections 同步由 worker pillar :: section context-assembly :: path claude-md-managed-sync 负责"
      :boundary "本 pillar 只管 global CLAUDE.md; 项目级归 worker context-assembly"))

  ;; ══════════════════════════════════════════════════════════
  ;; 5.4 Action-Instruction Specs — directive / plan / workflow 三段 prescription
  ;; ══════════════════════════════════════════════════════════
  (section action-instruction-specs
    :desc "所有描述'应该做什么 / 如何做'的规约 — DB 3 表 + Lisp/YAML 文件"
    :migrated-from "memory pillar :: project-management (4 tables) + non-db-forms (3 variants + 1 form) 于 v0.4.4"
    :rationale "memory 记'是什么'(facts); 本层记'应该做什么'(prescriptions) — 分层原则"
    :v0.4.19-note "DB 表 schema 归 memory :: module directive-layer; 本 section 负责 writer / compiler / sync 逻辑"

    (component directive-spec-db
      :desc "user utterance → lisp 指令编译记录 — 三段式 pipeline 第一段"
      :table "directive"
      :schema-owned-by "memory :: module directive-layer :: plumbing directive-compilation"
      :writer-owned-by "本 pillar (directive-compiler actor, TBD)"
      :status "schema-ready-pending-implementation"
      :v0.4.19-rename "原名 intent 表 → directive 表 (避命名歧义: 和 <project>/.missiond/intent.lisp 代码画像文件区分)"
      :vs-per-project-intent "memory :: project-management :: <project>/.missiond/intent.lisp = factual 代码快照; 本表 = 'Jarvis 对用户话的 lisp 指令编译'")

    (component plan-spec-db
      :desc "directive 编译出的执行 DAG — 绑 board_task + 版本 + FSM"
      :table "plan"
      :schema-owned-by "memory :: module directive-layer :: plumbing plan-execution"
      :writer-owned-by "本 pillar (plan-compiler actor, TBD)"
      :status "schema-ready-pending-implementation"
      :future-concerns ["plan 编译" "FSM 迁移" "supersede-chain 策略"])

    (component workflow-spec-db
      :desc "从成功 plan 蒸馏的可复用模板 — 带 match_rules + 统计"
      :table "workflow"
      :schema-owned-by "memory :: module directive-layer :: plumbing workflow-templates"
      :writer-owned-by "本 pillar (workflow-distiller actor, TBD)"
      :status "schema-ready-pending-implementation"
      :future-concerns ["distillation 算法" "匹配阈值" "LRU 策略"])

    (path directive-plan-workflow-chain
      :lifecycle-style "pending — 3 actor 全 TBD"
      :status "schema-ready 但无运行时实现"
      (ingress
        :source "用户 utterance (来自 Claude Code 对话) / 系统指令 / future actor / future sync worker"
        :entry-components ["directive-spec-db" "plan-spec-db" "workflow-spec-db"])
      (logic-core
        (step s1 "directive: 把用户语言编译成系统可理解的 lisp-level prescription (LLM-assisted)")
        (step s2 "plan: 从 directive 生成可执行 DAG / FSM / supersede chain")
        (step s3 "workflow: 从成功 plan 蒸馏出可复用模板 + match_rules")
        (step s4 "schema 存储由 memory :: directive-layer 持有 (pg/directive.rs 17 方法已存在 trait)")
        (step s5 "writer TBD: 三种候选 — (a) 本 pillar actor (推荐新建 directive-compiler) / (b) worker pillar 某 BackgroundWorker / (c) MCP 工具直写"))
      (egress
        :writes ["directive" "plan" "workflow (表, 未来)"]
        :reads ["board_tasks"]
        :via-bus []
        :memory-cross-ref ["directive-layer"]
        :returns "compiled prescription / reusable template"
        :tools-surface-future "mission_directive / mission_plan / mission_workflow (MCP tools, TBD)")

    (specs-manager-status
      :desc "action/instruction specs 的 read/write/reload manager"
      :actions ["read" "write" "reload" "sync-with-file"]
      :status "mostly TBD — DB 3 表是 schema-only, 无 Rust writer 实现; 文件层已有 readers (mission_intent, flow-engine-v2 loader)"
      :cross-ref "memory :: project-management :: path project-code-snapshot (读 per-project 代码快照 FILE, 职责不同)"
      :future-decision "要么实现 3 DB 表的 writer actor, 要么下次 migration DROP 以消除 dead schema"))

  ;; ══════════════════════════════════════════════════════════
  ;; 5.5 Workflows — methodology lisp + executable YAML
  ;; ══════════════════════════════════════════════════════════
  (section workflows
    :desc "统一工作流规约 — 两 kind (人类看 vs 机器跑), 概念一致格式异"
    :unified-in "v0.4.5 (原 workflow-lisp-templates + flow-yaml-templates 合并)"
    :design-rationale "两 kind 形式差异大但概念一致 — 保留各自格式优势, 统一纳管"

    (kind methodology
      :desc "Lisp 方法论模板 — 人类 / agent 参考, 非运行时执行"
      :path ".missiond/workflows/*.lisp"
      :consumers ["human" "mission_intent tool" "agent 参考"]
      :granularity "抽象叙事 — phases / principles / anti-patterns / baseline-numbers / decision-authority"
      :examples ["pillar-refactor.lisp (335 行, 本会话凝结 memory pillar 重构方法论)"
                 "bus-refactor.lisp (11-phase 事件总线重构方法论)"]
      :executability "✗ 非运行时执行, 纯文档"
      :owner "本 pillar")

    (kind executable
      :desc "YAML 声明式节点编排 — flow-engine-v2 运行时执行"
      :path "$MISSIOND_HOME/flows/*.yaml"
      :loader "crates/missiond-daemon/src/engine/flow/loader.rs (worker pillar 实现)"
      :executor "worker pillar :: section engine-cluster :: subsection flow-engine-v2"
      :parser "serde_yaml::from_str::<FlowDefinition>"
      :granularity "具体机器操作 — 5 node types: LlmCall / SlotTask / McpTool / DaemonAction / ParallelSlotTasks"
      :executability "✓ 机器执行"
      :owner "本 pillar (定义 SSOT) + worker pillar (runtime 执行)")

    (path workflow-kinds-split
      :lifecycle-style design-time
      (ingress
        :source "人要看方法论 / 机器要跑 workflow"
        :entry-components [".missiond/workflows/*.lisp" "$MISSIOND_HOME/flows/*.yaml"])
      (logic-core
        (step s1 "methodology-lisp: phases / principles / anti-patterns / authority 边界 (人类消费)")
        (step s2 "executable-yaml: 具体机器节点序列 (flow-engine-v2 消费)")
        (step s3 "两者概念统一为 workflow, 按 受众 / 粒度 / 执行性 拆分")
        (step s4 "允许同名映射 (如 bus-refactor.lisp ↔ bus-refactor.yaml), 不强制 1:1")
        (step s5 "未来若需, 可由 forge 把 methodology-lisp 冲压为 executable-yaml (SSOT Lisp + 冲压副本)"))
      (egress
        :writes []
        :reads []
        :returns "human methodology / machine workflow"
        :consumed-by ["flow pillar (SSOT 索引)" "worker::flow-engine-v2 (执行)" "human / agent review (方法论)"])

    (relationship-between-kinds
      :overlap "都描述'多步工作流'"
      :split-axis "受众 (human vs machine) + 粒度 (抽象 vs 具体) + 执行性"
      :why-not-unify-format "Lisp 富元数据给人看, YAML 轻量 schema 给 flow-engine 消费; 硬统一两边都难用"
      :cross-ref-convention "可约定同名对照, 非强制"
      :future-possibility "若需要, 可用 Forge 从 Lisp 冲压 YAML, 当前不做"))

  ;; ══════════════════════════════════════════════════════════
  ;; 5.6 LispSurvey — 项目代码变化感知器 (dual-owned with worker)
  ;; ══════════════════════════════════════════════════════════
  (section lisp-survey-dual-owned
    :desc "项目代码变化 → <project>/.missiond/intent.lisp 增量更新"
    :worker-cross-ref "worker pillar :: section worker-cluster :: subsection worker-sonnet :: path lisp-survey-update"
    :ownership-split
      (trigger-on-worker "ContextualCommitDetected subscribe + BackgroundWorker impl + self-trigger filter + debounce")
      (semantic-on-this-pillar "survey prompt 模板 + slot registered-task 'lisp_survey' + intent.lisp 写 ownership")

    (path lisp-survey-update-semantic
      :lifecycle-style event-driven
      :added "commit 79a877f"
      (ingress
        :source "worker pillar 的 lisp-survey-update path 触发后, slot 端执行本 path 语义"
        :entry-components
          ["crates/missiond-daemon/src/workers/sonnet/lisp_survey_worker.rs (触发侧, worker 拥有)"
           "slot_orchestrator registered-task 'lisp_survey' (slot-id lisp-surveyor, model sonnet, timeout 900s)"
           "survey prompt (本 pillar 拥有的模板)"])
      (logic-core
        (step s1 "worker 侧已过滤 self-trigger (slot_id != lisp-surveyor) + ProjectRegistry::resolve + 60s debounce")
        (step s2 "本 pillar 组装 survey prompt: 'diff + intent_path + 差量 指令'")
        (step s3 "slot_manager.execute('lisp_survey', prompt) → lisp-surveyor persistent Sonnet slot")
        (step s4 "slot 端执行 LLM 调用 + 解析 response")
        (step s5 "如 NO_CHANGE: 跳过; 否则 slot Edit 工具更新 <project>/.missiond/intent.lisp"))
      (egress
        :writes []
        :reads []
        :file-writes ["<project>/.missiond/intent.lisp (via slot Edit tool)"]
        :via-bus ["SystemEvent::ContextualCommitDetected (consumed on worker side)"]
        :returns "survey result / NO_CHANGE / intent file update"))

  ;; ══════════════════════════════════════════════════════════
  ;; 5.7 ArchMaintenance — 架构文档演化器 (dual-owned with worker)
  ;; ══════════════════════════════════════════════════════════
  (section arch-maintenance-dual-owned
    :desc "代码 commit → architecture manifest 增量更新"
    :worker-cross-ref "worker pillar :: section worker-cluster :: subsection worker-sonnet :: path arch-maintenance-worker-cycle"
    :ownership-split
      (trigger-on-worker "ContextualCommitDetected subscribe (commit 65c8b59 替换 git-log 3600s polling)")
      (semantic-on-this-pillar "arch prompt 模板 + slot registered-task 'arch_maintenance' + manifest 文件 ownership")

    (path arch-maintenance-semantic
      :lifecycle-style event-driven
      (ingress
        :source "worker 侧 arch_maintenance_worker 触发后, slot 端执行本 path 语义"
        :entry-components
          ["crates/missiond-daemon/src/workers/sonnet/arch_maintenance_worker.rs (触发, worker)"
           "slot_orchestrator registered-task 'arch_maintenance' (slot-id arch-surveyor, model sonnet, timeout 900s)"
           "arch prompt (本 pillar 拥有的模板)"])
      (logic-core
        (step s1 "消费带 conversation/session/slot 上下文的 ContextualCommitDetected (commit 65c8b59 新事件 — 含 conversation_id 桥接)")
        (step s2 "本 pillar 组装 arch maintenance prompt: 'diff + project + context + manifest 位置'")
        (step s3 "SlotManager.execute('arch_maintenance', prompt) → arch-surveyor slot")
        (step s4 "slot 端执行 LLM 调用 + Edit 更新 architecture manifest 文件")
        (step s5 "manifest 文件归本 pillar ownership, 非 memory"))
      (egress
        :writes []
        :reads []
        :file-writes ["project architecture manifest (例: YAML 或 Markdown, 项目自定义)"]
        :via-bus ["SystemEvent::ContextualCommitDetected (consumed on worker side)"]
        :returns "slot dispatch result / arch manifest update"))

  ;; ══════════════════════════════════════════════════════════
  ;; 5.8 Learning Engine — 7 sub-engine 认知/推理内核
  ;; ══════════════════════════════════════════════════════════
  (section learning-engine
    :desc "learning-engine 7 sub-engine — 从 worker pillar 迁入 (DC014 决策)"
    :migrated-from "worker pillar :: engine-cluster :: learning-engine"
    :remaining-in-worker "触发点 — BackgroundWorker impl + event subscribe + lifecycle mechanics"
    :primary-ownership-here "认知逻辑 / 推理 pipeline / 分析模型"
    :code-files-location "仍在 crates/missiond-daemon/src/engine/learning_engine/ (物理位置不变, 语义归属迁移)"
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
    :support-files ["gen_engine.rs — Forge domain shell, 零业务逻辑"]

    ;; ── 5.8.1 Decision sub ──
    (subsection decision
      :desc "决策识别 + 模式归纳"
      :purpose "识别问题是否可由 KB 直接解 / 需要 LLM 辅助 / 应升级人类; 从历史决策中归纳可复用模式"

      (path decision-cascade
        :lifecycle-style event-driven
        :target "crates/missiond-daemon/src/engine/learning_engine/decision_engine.rs"
        :phase-B-verified "2026-04-21 — 4 tier 全部已实现 (详 phase-B-scan-findings § A.3)"
        (ingress
          :source "agent question / decision need / escalation path"
          :entry-components ["decision_engine.rs"])
        (logic-core
          :cascade
            [(tier-1 kb-lookup
                :status "✓ implemented @ decision_engine.rs:155-180"
                :impl "kb_search_ranked() + dual scoring + confidence ≥0.5 threshold for auto-apply")
             (tier-2 gemini-consult
                :status "✓ implemented @ decision_engine.rs:210-260"
                :impl "call_gemini_for_flow() + JSON contract (answer/reasoning/action) + confidence 0.7")
             (tier-3 decision-slot
                :status "✓ implemented @ decision_engine.rs:290-340"
                :impl "request_execution_slot(slot-decision) + pty.send(120s timeout)")
             (tier-4 human-escalation
                :status "✓ implemented @ decision_engine.rs:360-380"
                :impl "agent_questions (target=master) + board_task priority 升级")])
        (egress
          :writes ["agent_questions" "question_routing_trace"]
          :reads ["agent_questions (pending)" "kb_entries (search_ranked/get_by_id)" "board_tasks" "board_task_notes"]
          :via-bus ["QuestionEvent::Resolved" "QuestionEvent::DecisionResolved" "TaskEvent::Created"]
          :llm-calls "Gemini (tier-2)"
          :slot-calls "slot-decision (tier-3 dispatch via pty.send)"
          :returns "decision result (tier / answer / needs-human)"))

      (path decision-harvest-generalization
        :lifecycle-style spawned
        :target "crates/missiond-daemon/src/engine/learning_engine/decision_harvest.rs"
        (ingress
          :source "已发生的 decision / outcome 样本"
          :entry-components ["decision_harvest.rs"])
        (logic-core
          (step s1 "回看历史决策 + 对应 outcome")
          (step s2 "抽取可复用模式 / policy 候选")
          (step s3 "将模式化结果回送给知识 / 策略侧使用"))
        (egress
          :writes []
          :reads []
          :via-bus []
          :returns "harvested decision patterns"
          :need-more-ground-truth "durable sink 未明")))

    ;; ── 5.8.2 Extraction sub ──
    (subsection extraction
      :desc "事件/会话 → 知识提取"
      :state-machine-crossref "extraction-phase FSM (见 5.11)"

      (path extraction-pipeline
        :lifecycle-style spawned
        :target "crates/missiond-daemon/src/engine/learning_engine/extraction.rs"
        :phases-fsm "Idle → Sending → WaitingForIdleness → Complete (引用 5.11)"
        (ingress
          :source "session/event 落地后的知识提取触发"
          :entry-components ["extraction.rs"])
        (logic-core
          (step s1 "从 conversation / event material 中抽取候选知识")
          (step s2 "执行快速提取 (快 path)")
          (step s3 "执行深度提取 (slow path, 通常经 slot)")
          (step s4 "知识候选返回给上层消费链路 (KB writer)"))
        (egress
          :writes []
          :reads ["conversations" "conversation_messages"]
          :via-bus []
          :returns "knowledge extraction candidates"
          :need-more-ground-truth "精确 table sink 待 phase-B"))

      (path historical-scan-backfill
        :lifecycle-style spawned
        :target "crates/missiond-daemon/src/engine/learning_engine/historical_scanner.rs"
        (ingress
          :source "回溯扫描请求 / backlog catch-up"
          :entry-components ["historical_scanner.rs"])
        (logic-core
          (step s1 "按时间窗口或 session backlog 扫历史会话")
          (step s2 "把旧数据补进 extraction / analysis 队列")
          (step s3 "扫描结果交给后续提取器"))
        (egress
          :writes []
          :reads []
          :via-bus []
          :returns "historical scan result"))

      (path idle-explore-trigger
        :lifecycle-style spawned
        :target "crates/missiond-daemon/src/engine/learning_engine/idle_explorer.rs"
        (ingress
          :source "系统或 slot 空闲期触发"
          :entry-components ["idle_explorer.rs"])
        (logic-core
          (step s1 "检测系统/slot 的空闲窗口")
          (step s2 "空闲窗口内触发 exploration / extraction / backfill 任务")
          (step s3 "待探索对象交后续 path, 自己只做时机治理"))
        (egress
          :writes []
          :reads []
          :via-bus []
          :returns "idle exploration triggers")))

    ;; ── 5.8.3 Analysis sub ──
    (subsection analysis
      :desc "意图分析 + 时间轴分析"

      (path intent-analysis
        :lifecycle-style spawned
        :target "crates/missiond-daemon/src/engine/learning_engine/intent_analyst.rs"
        :唯一明确写表 "本 sub-engine 是 learning-engine 7 个中唯一在 memory v0.5.1 frozen 有明确 writer 声明的"
        :memory-cross-ref-binds-to "memory :: module conversation-logs :: binds-to intent_analyst"
        (ingress
          :source "session / turn 分析触发 + autopilot get_recent_intents 也调"
          :entry-components ["intent_analyst.rs"])
        (logic-core
          (step s1 "按 session 扫 conversation turns")
          (step s2 "用分析模型为一组 turns 识别 intent group")
          (step s3 "把 intent group 与 turn back-reference 写回 conversation 视角"))
        (egress
          :writes ["user_intents" "conversation_turns.intent_group_id"]
          :reads ["conversation_turns"]
          :via-bus []
          :memory-cross-ref ["conversation-logs"]
          :trait-writer "ConversationStore::insert_user_intent + 5 查询方法 (traits.rs:136-150)"
          :returns "intent analysis result"))

      (path timeline-analysis
        :lifecycle-style spawned
        :target "crates/missiond-daemon/src/engine/learning_engine/timeline_analyst.rs"
        (ingress
          :source "时间轴 / long-range sequence 分析触发"
          :entry-components ["timeline_analyst.rs"])
        (logic-core
          (step s1 "按时间顺序聚合会话 / 事件 / 任务序列")
          (step s2 "寻找长期模式 / 关键转折点 / 上下文压缩机会")
          (step s3 "分析结果回给策略、记忆或展示侧"))
        (egress
          :writes []
          :reads []
          :via-bus []
          :returns "timeline analysis result"
          :need-more-ground-truth "precise table contract 待 phase-B")))

    (learning-engine-contract-summary
      :phase-B-verified "2026-04-21 — 7 sub R/W 矩阵全 confirmed (详 phase-B-scan-findings § A.1)"
      :writes-full-matrix
        (decision_engine     :writes ["agent_questions (answer/retry_count)" "question_routing_trace"])
        (decision_harvest    :writes ["kb_entries (policy:decision category)" "kb_update (confidence)"])
        (extraction          :writes ["realtime_forwarded_at (watermarks)" "deep_checkpoint" "slot_tasks" "conversation_task_id" "kb_update" "kb_forget"])
        (historical_scanner  :writes ["mark_habit_scanned" "daemon_state (last_habit_scan_at)"])
        (idle_explorer       :writes ["board_tasks (create, auto_execute=true)" "kb_remember" "daemon_state"])
        (intent_analyst      :writes ["user_intents" "conversation_turns.intent_group_id"])
        (timeline_analyst    :writes ["board_tasks (create)" "kb_remember (category=ops:insight)" "daemon_state"])
      :reads-full-matrix
        (decision_engine     :reads ["agent_questions (pending)" "kb_entries" "board_tasks" "board_task_notes"])
        (decision_harvest    :reads ["agent_questions (target=master)"])
        (extraction          :reads ["pending_realtime_messages" "conversation status" "pending_deep_analysis conversations" "conversation_messages" "daemon_state" "kb_list_low_utility"])
        (historical_scanner  :reads ["daemon_state" "conversation counts" "unscanned_conversations"])
        (idle_explorer       :reads ["daemon_state (last_idle_explore_at)" "kb_entries" "board_tasks" "beacons" "snapshots"])
        (intent_analyst      :reads ["conversation_turns (intent_coverage, turns_after)" "caller source"])
        (timeline_analyst    :reads ["timeline_stats (12h)" "timeline_search (errors)" "board_tasks (dedup)"])
      :llm-usage
        (gemini ["decision_engine (tier-2)" "decision_harvest (Few-Shot)" "timeline_analyst (12h insight JSON)"])
        (sonnet ["extraction (KB reflection)" "intent_analyst (intent pattern detection)"])
      :slot-usage
        ["decision_engine → slot-decision (tier-3)"
         "extraction → request_default_slot + request_execution_slot (300-900s)"
         "historical_scanner → MEMORY_SLOW_SLOT_ID"]
      :via-bus-produces
        ["QuestionEvent::Resolved/DecisionResolved (decision)"
         "SlotEvent::TaskDispatched (extraction/scanner)"
         "MemoryEvent::PhaseChanged (extraction)"
         "MemoryEvent::IntentAnalyzed (intent_analyst)"
         "BoardEvent::StatusChanged (idle_explorer)"
         "SystemEvent::InsightGenerated (timeline)"]
      :sub-engine-details
        (idle_explorer :8-categories ["一致性" "陈旧" "信标" "重复" "聚合" "巩固" "状态" "影子回放"])
        (intent_analyst :limits "15 轮批量 (防翻页 bug)" :patterns ["stuck_retry" "architecture_explore" "refactor_shift" "scope_creep"])
        (historical_scanner :interval "4h 周期")
        (timeline_analyst :interval "12h 周期")))

  ;; ══════════════════════════════════════════════════════════
  ;; 5.9 Flow-Engine v1 — Project Lifecycle Phases
  ;; ══════════════════════════════════════════════════════════
  (section flow-engine-v1-project-lifecycle
    :desc "flow-engine v1: project lifecycle phases 推进 (从 worker 迁入, DC014)"
    :migrated-from "worker pillar :: engine-cluster :: intent-engine :: board-phase-engine"
    :target "crates/missiond-daemon/src/engine/intent_engine/flow_engine.rs"
    :distinguish-from-v2 "v2 是 YAML declarative general-purpose runtime, 归 worker pillar; v1 是 autopilot-driven project-lifecycle phases, 归本 pillar"
    :state-machine-crossref "engineering-phase FSM (见 5.11)"

    (path board-phase-engine
      :lifecycle-style spawned
      :phase-B-verified "2026-04-21 — 7 phase 全部已实现 + decision_harvest closure loop (详 phase-B-scan-findings § A.4)"
      :completeness "✓ FULL — Investigate/ConsultGemini1/Plan/ConsultGemini2/Execute/Finalize/Done 全 transition 有代码 + artifact 存储"
      (ingress
        :source "autopilot claim 后的 board task phase progression"
        :entry-components
          ["crates/missiond-daemon/src/engine/intent_engine/flow_engine.rs"
           "crates/missiond-daemon/src/engine/intent_engine/autopilot.rs (worker pillar 触发)"])
      (logic-core
        (step s1 "按 board lifecycle phase 把任务从 Investigate / ConsultGemini1 / Plan / ConsultGemini2 / Execute / Finalize / Done 推进")
        (step s2 "每 phase 转换需对应 mission_submit_phase_result 提交的 artifactType (investigation_report / gemini_advice_1 / execution_plan / gemini_advice_2 / execution_result / commit_hash)")
        (step s3 "ConsultGemini1/2 阶段 daemon 直调 Gemini (非 slot), call_gemini_for_flow + JSON 契约解析")
        (step s4 "Investigate/Plan/Execute/Finalize 阶段走 slot (pty.send)")
        (step s5 "Finalize→Done 自动 trigger decision_harvest → policy:decision KB 条目沉淀")
        (step s6 "re-entry guard 防并发; phase 推进结果与失败状态回写 board_tasks.flow_phase"))
      (transitions-full-implementation
        ((Investigate -> ConsultGemini1 :at "flow_engine.rs:150-200" :artifact "investigation_report")
         (ConsultGemini1 -> Plan         :at "flow_engine.rs:200-250" :artifact "gemini_advice_1")
         (Plan -> ConsultGemini2         :at "flow_engine.rs:250-300" :artifact "execution_plan")
         (ConsultGemini2 -> Execute      :at "flow_engine.rs:300-350" :artifact "gemini_advice_2")
         (Execute -> Finalize            :at "flow_engine.rs:350-400" :artifact "execution_result" :note "with error handling + retry guard")
         (Finalize -> Done               :at "flow_engine.rs:400-450" :artifact "commit_hash + decision_harvest trigger")))
      (egress
        :writes ["board_tasks.status" "board_tasks.flow_phase" "board_tasks.flow_context" "board_tasks.flow_artifacts" "board_task_notes"]
        :reads ["board_task (flow_phase, flow_context, questions, notes)" "flow_template definitions"]
        :via-bus ["TaskEvent::PhaseChanged"]
        :llm-calls "Gemini (ConsultGemini1/2 direct, 非 slot)"
        :slot-calls "pty.send for Investigate/Plan/Execute/Finalize phases"
        :memory-cross-ref ["board"]
        :tools-surface "mission_submit_phase_result (sysinfra domain via misc handler)"
        :returns "board phase progression result"
        :closure-with-learning-engine "Finalize→Done auto-harvests decisions into policy:decision KB 条目 (→ learning-engine decision_harvest 闭环)"))

  ;; ══════════════════════════════════════════════════════════
  ;; 5.10 Action-Instruction Actor (Future)
  ;; ══════════════════════════════════════════════════════════
  (section action-instruction-actor
    :desc "directive / plan / workflow 编译器 actor — 当前全 TBD"
    :status "schema-ready-pending-implementation"

    (actor directive-compiler
      :status TBD
      :desc "把用户 utterance (自然语言) 编译成 lisp-level directive prescription"
      :future-target "crates/missiond-daemon/src/intent_layer/directive_compiler.rs (TBD)"
      :input "用户话 / 系统指令"
      :output "directive record → directive 表 (memory :: directive-layer writer)"
      :dependencies ["LLM (gemini/sonnet chat)" "context assembly" "KB (convention 查询)"])

    (actor plan-compiler
      :status TBD
      :desc "从 directive 生成可执行 DAG / FSM / supersede chain"
      :future-target "crates/missiond-daemon/src/intent_layer/plan_compiler.rs (TBD)"
      :input "directive record"
      :output "plan record → plan 表"
      :dependencies ["directive-compiler 输出" "board_tasks 模板"])

    (actor workflow-distiller
      :status TBD
      :desc "从成功 plan 蒸馏出可复用 workflow 模板"
      :future-target "crates/missiond-daemon/src/intent_layer/workflow_distiller.rs (TBD)"
      :input "已完成的 plan + outcome"
      :output "workflow record → workflow 表"
      :dependencies ["plan 历史" "match_rules 算法"])

    (mcp-tools-future
      :mission_directive "TBD — directive 编译 / 查询 / 更新"
      :mission_plan      "TBD — plan 查询 / 手动 supersede"
      :mission_workflow  "TBD — workflow 模板管理")

    (mcp-tools-existing-crossref
      :mission_execution "worker v0.3 I007 相关 — 12 actions handler 实现 (agent-execution-coordination v0.5.1 manager-interface)"
      :cross-ref-memory "memory v0.5.1 helper agent-execution-coordination"))

  ;; ══════════════════════════════════════════════════════════
  ;; 5.11 State Machines Owned — engineering-phase + extraction-phase
  ;; ══════════════════════════════════════════════════════════
  (section state-machines-owned
    :desc "归本 pillar 的 FSM — 元层语义"
    :authority "intent-pillar-state-machines.lisp 老图"

    (state-machine engineering-phase
      :target "crates/missiond-core/src/types/board.rs (enum EngineeringPhase)"
      :owner-path "本 pillar :: flow-engine-v1-project-lifecycle"
      :consumed-by ["flow-engine v1 :: board-phase-engine" "mission_submit_phase_result"]

      (states
        (Investigate "调查阶段 — agent 研究代码 / 需求")
        (Consult     "审查阶段 — Gemini / 专家审核调查结果")
        (Plan        "计划阶段 — 生成执行计划")
        (Execute     "执行阶段 — 实际动工")
        (Finalize    "收尾阶段 — commit / review / 复盘"))

      (transitions
        (Investigate -> Consult   :trigger "context-gathered")
        (Consult     -> Plan      :trigger "review-complete")
        (Plan        -> Execute   :trigger "plan-approved")
        (Execute     -> Finalize  :trigger "implementation-done")
        (Finalize    -> Investigate :trigger "issues-found")))

    (state-machine extraction-phase
      :target "crates/missiond-daemon/src/engine/learning_engine/extraction.rs"
      :owner-path "本 pillar :: learning-engine :: subsection extraction :: path extraction-pipeline"

      (states
        (Idle              "空闲等待")
        (Sending           "发送 extraction 请求 (通常派 slot)")
        (WaitingForIdleness "等待 slot 回到 idle")
        (Complete          "提取完成"))

      (transitions
        (Idle              -> Sending            :trigger "extraction-triggered")
        (Sending           -> WaitingForIdleness :trigger "content-sent")
        (WaitingForIdleness -> Complete          :trigger "slot-idle")
        (WaitingForIdleness -> Idle              :trigger "timeout")
        (Complete          -> Idle               :trigger "reset")))

    (related-fsm-other-pillars
      :note "其他 FSM 归属非本 pillar"
      (pty-session         :归属 "worker pillar :: section pty :: subsection pty-state-machine (8 states + 14 transitions)")
      (board-task          :归属 "memory pillar :: module board (BoardTaskStatus enum)")
      (task                :归属 "memory pillar :: module system-support")
      (question            :归属 "memory pillar :: module system-support")))

  ;; ══════════════════════════════════════════════════════════
  ;; Design Principles
  ;; ══════════════════════════════════════════════════════════
  (design-principles
    (principle-1 "intent-layer 管 prescriptive: 系统应该怎么做")
    (principle-2 "memory 管 factual: 系统现在记得什么 / 代码真实状态")
    (principle-3 "worker 管 runtime: 系统在跑什么 (timer / dispatch / event)")
    (principle-4 "execution-log 属 operational state (memory v0.5.1 helper), 不和 methodology 混写")
    (principle-5 "forge + mission_intent = 本 pillar 对外两大出口")
    (principle-6 "双重归属 = 触发/语义 拆分 (lisp_survey / arch_maintenance 模式)")
    (principle-7 "认知/推理逻辑 归本 pillar; 运行时/调度 机制 归 worker pillar"))

  ;; ══════════════════════════════════════════════════════════
  ;; Need-more-ground-truth (IL-T001 …)
  ;; ══════════════════════════════════════════════════════════
  (need-more-ground-truth
    (IL-T001 :status RESOLVED :resolved-at "2026-04-21"
             :finding "7 sub R/W 矩阵全 confirmed. 详 phase-B-scan-findings-2026-04-21.md § A.1 + learning-engine-contract-summary 已补 full matrix")
    (IL-T002 :status "awaiting-decision"
             :note "3 actor (directive-compiler / plan-compiler / workflow-distiller) 真实实现时机 — 指挥官决策")
    (IL-T003 :status "awaiting-decision"
             :note "mission_directive / mission_plan / mission_workflow MCP tool 实现时机")
    (IL-T004 :status "awaiting-decision"
             :note "global-claudemd-manager daemon 侧实现 (mission_global_instruction) 时机")
    (IL-T005 :status "future-implementation"
             :note "mission_execution (agent-execution-coordination v0.5.1, 12 actions) handler — 同步 worker I007")
    (IL-T006 :status "pending-external-scan"
             :note "forge-daemon/src/intent_graph.rs 在外部仓 ~/Projects/jarvis-forge, 本次 phase-B 未扫")
    (IL-T007 :status RESOLVED :resolved-at "2026-04-21"
             :finding "4 tier 全部已实现 (kb-lookup/gemini-consult/decision-slot/human-escalation). 详 § A.3 + decision-cascade path 已补 :status implemented")
    (IL-T008 :status "future-design"
             :note "workflow-distiller match_rules + LRU 策略 — actor 实现时设计")
    (IL-T009 :status RESOLVED :resolved-at "2026-04-21"
             :finding "flow-engine v1 EngineeringPhase 7 phase 全部实现 + 到 Finalize→Done 自动 trigger decision_harvest 形成闭环. 详 § A.4 + board-phase-engine path 已补 transitions-full-implementation 表")
    (IL-T010 :status "awaiting-decision"
             :note "未来 forge 冲压 methodology-lisp → executable-yaml (当前人工) — 决策")
    (IL-T011 :status "future-design"
             :note "本 pillar 独立 crate vs 嵌入 (当前嵌入 missiond-daemon)"))
  ;; close sections 5.2-5.11 (6 unclosed) + close pillar
  ))))))))
