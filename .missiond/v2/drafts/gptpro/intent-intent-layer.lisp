;; ══════════════════════════════════════════════════════
;; MissionD — Intent Layer (Detailed Draft)
;; 目标: 把“系统如何描述自己 / 如何被指挥 / 如何演化”从导航摘要展开成可施工结构
;; ══════════════════════════════════════════════════════

(pillar intent-layer
  :status "草稿 v0.1 — 以 v2 intent 总纲为主, 结合现有 lisp 生态与 worker/tool 实况"
  :primary-sources ["v2/intent.lisp :: pillar intent-layer" "所有 intent*.lisp 文件本身"]
  :secondary-sources ["intent-pillar-event-workers.lisp :: lisp-survey-worker"
                      "intent-pillar-mcp-dispatch.lisp :: mission_intent / mission_forge_*"
                      "intent-memory.lisp :: module directive-layer"]

  (purpose "元层: 系统如何描述自己, 如何感知变化, 如何冲压代码, 如何承载全局与项目级规约")

  (pillar-ingress
    (entry-1 "人类编写或修改的 lisp / markdown / yaml 规约文件")
    (entry-2 "ContextualCommitDetected 驱动的项目代码变化")
    (entry-3 "mission_intent / mission_forge_* / future mission_execution 工具调用")
    (entry-4 "全局 ~/.claude/CLAUDE.md 与项目级 intent / memory 文件"))

  (pillar-core
    (core-1 "intent files = 系统自描述语料")
    (core-2 "forge = Lisp → IR → Rust 的冲压器")
    (core-3 "governance = 模式声明 / lint / file-level 约束")
    (core-4 "action-instruction specs = directive / plan / workflow 的 prescription 层")
    (core-5 "lisp-survey-worker = 项目代码变化到 intent 快照更新的感知器"))

  (pillar-egress
    (egress-1 "更新后的 design lisp / project intent.lisp")
    (egress-2 "forge 生成的 Rust 代码 / lint 结果")
    (egress-3 "供 worker / tools / flow 消费的 methodology 与 executable workflow")
    (egress-4 "供人和 agent 读取的 summary / section / graph"))

  ;; ─────────────────────────────────────────────────────
  ;; 自描述文件生态
  ;; ─────────────────────────────────────────────────────
  (path system-intent-read
    (ingress
      :source "mission_intent(read/section/summary/list) / 人工打开 .missiond/v2/*.lisp"
      :entry-components ["system-level intent files" "mission_intent handler"])
    (logic-core
      (step s1 "按 project 或系统域定位目标 lisp 文件")
      (step s2 "读取全文或切 section block")
      (step s3 "必要时做 summary 或 list views")
      (step s4 "将结果以 raw lisp / block / summary 形式返回")
      (step s5 "保留对 file path 与 survey-date 的可追溯信息"))
    (egress
      :returns "intent raw content / section / summary / file list"))

  (path intent-graph-build
    (ingress
      :source "lisp 文件集合发生变化 / 需要可视化治理"
      :entry-components ["intent-graph"])
    (logic-core
      (step s1 "扫描文件间的 module-link / cross-ref / target 指向")
      (step s2 "构建有向图")
      (step s3 "标出 orphan / cycle / broken-link / duplicate-target")
      (step s4 "供 governance 或可视化消费")
      (step s5 "输出 graph summary 或图数据"))
    (egress
      :returns "graph / governance diagnostics"))

  ;; ─────────────────────────────────────────────────────
  ;; 指挥规约层
  ;; ─────────────────────────────────────────────────────
  (path global-claudemd-load
    (ingress
      :source "Claude Code 会话启动 / future mission_global_instruction 工具"
      :entry-components ["~/.claude/CLAUDE.md" "global-claudemd-manager (future)"])
    (logic-core
      (step s1 "读取全局 CLAUDE.md")
      (step s2 "解析 Markdown + 可选 frontmatter")
      (step s3 "把全局偏好、约束、宇宙总纲注入系统提示")
      (step s4 "与项目级 CLAUDE.md / intent / memory 形成分层")
      (step s5 "未来可通过 manager 做 read/edit/reload"))
    (egress
      :returns "global instruction context"))

  (path directive-plan-workflow-chain
    (ingress
      :source "用户 utterance / 系统指令 / future actor / future sync worker"
      :entry-components ["directive-spec-db" "plan-spec-db" "workflow-spec-db"])
    (logic-core
      (step s1 "directive: 把用户语言编译成系统可理解的 lisp-level prescription")
      (step s2 "plan: 从 directive 生成可执行 DAG / FSM / supersede chain")
      (step s3 "workflow: 从成功 plan 蒸馏出可复用模板")
      (step s4 "schema 与存储由 memory::directive-layer 持有")
      (step s5 "writer 仍 TBD: actor / worker / MCP 工具三种候选"))
    (egress
      :writes "directive / plan / workflow 三表 (未来)"
      :returns "compiled prescription / reusable template"))

  ;; ─────────────────────────────────────────────────────
  ;; 演化与冲压
  ;; ─────────────────────────────────────────────────────
  (path forge-compilation
    (ingress
      :source "mission_forge_build / mission_forge_lint / build-time forge CLI"
      :entry-components ["forge" "governance"])
    (logic-core
      (step s1 "读取系统或项目 intent lisp")
      (step s2 "按 governance 模式校验: strict-codegen / descriptive / experimental")
      (step s3 "Lisp → IR")
      (step s4 "IR → Rust generation gap 输出")
      (step s5 "返回 build 或 lint 结果"))
    (egress
      :returns "generated code / lint report / violations_raw"))

  (path lisp-survey-update
    (ingress
      :source "ContextualCommitDetected"
      :entry-components ["lisp-survey-worker"])
    (logic-core
      (step s1 "拿到 project_id / slot_id / commit_hash / diff")
      (step s2 "过滤 survey worker 自触发")
      (step s3 "ProjectRegistry 解析 intent_path")
      (step s4 "按项目 debounce")
      (step s5 "调用 survey slot 依据代码 diff 更新 project intent snapshot"))
    (egress
      :writes "<project>/.missiond/intent.lisp"
      :returns "survey result / no-change"))

  ;; ─────────────────────────────────────────────────────
  ;; 方法论与可执行工作流
  ;; ─────────────────────────────────────────────────────
  (path workflow-kinds-split
    (ingress
      :source "人要看方法论 / 机器要跑 workflow"
      :entry-components [".missiond/workflows/*.lisp" "$MISSIOND_HOME/flows/*.yaml"])
    (logic-core
      (step s1 "methodology-lisp 承载 phases / principles / anti-patterns / authority 边界")
      (step s2 "executable-yaml 承载具体机器节点序列")
      (step s3 "两者概念统一为 workflow, 但按受众 / 粒度 / 执行性拆分")
      (step s4 "允许同名映射, 但不强制一一对应")
      (step s5 "未来若需要, 可由 forge 把 methodology-lisp 冲压为 executable-yaml"))
    (egress
      :returns "human methodology / machine workflow"
      :consumed-by ["flow pillar" "worker::flow-engine-v2" "human / agent review"]))

  (path governance-lint
    (ingress
      :source "forge lint / 人工审查 / future intent graph audit"
      :entry-components ["governance"])
    (logic-core
      (step s1 "检查模式违规、命名漂移、broken targets、不一致 cross-ref")
      (step s2 "把 descriptive / strict-codegen / experimental 分层对齐")
      (step s3 "输出 violations 与建议修复项")
      (step s4 "必要时阻止 build 或提示人工批准")
      (step s5 "沉淀为长期治理规则"))
    (egress
      :returns "lint report / governance recommendation"))

  (design-principles
    (principle-1 "intent-layer 管 prescriptive 东西: 系统应该怎么做")
    (principle-2 "memory 管 factual 东西: 系统现在记得什么 / 代码真实状态是什么")
    (principle-3 "execution-log 属于 operational state, 不应和 methodology 混写")
    (principle-4 "forge 与 mission_intent 是 intent-layer 对外最重要的两个出口"))
)
