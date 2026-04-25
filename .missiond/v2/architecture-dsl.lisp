;; ═════════════════════════════════════════════════════════════
;; MissionD — Reusable Architecture DSL (architecture-v1)
;; 目标: 把"pillar / function / flow 都有 ingress → logic-core → egress"
;;       升格为可复用、可检查、可生成图的 Lisp DSL。
;; 适用: MissionD v2 架构 Lisp, 以及未来其他程序的架构 Lisp。
;; ═════════════════════════════════════════════════════════════

(defdsl architecture-v1
  :version "v0.3"
  :status "declarative schema 2026-04-26 — reader/checker first; v0.2 adds source-index taxonomy (precompression); v0.3 adds execution handoff dual-plane rule"
  :checker "scripts/check-architecture-lisp.mjs"

  (purpose
    "声明架构 Lisp 的通用语法骨架; 第一阶段只做 parse + shape validation, 不做代码生成"
    "v0.2 扩: 在 主 Lisp 真正压缩/拆分前, 先固定 section-id / status / split / compression 约定,"
    "        让 pillar source index 可机器读, 后续压缩不丢 cross-ref"
    "v0.3 扩: 执行型 flow 可声明 control-plane / durability-plane handoff, 防止 operational report 与代码成果脱节")

  (reader-contract
    :syntax "S-expression + bracket vector"
    :comments "; to end-of-line"
    :strings "double-quoted strings with backslash escapes"
    :balanced-delimiters ["()" "[]"]
    :non-goal "不要求兼容 Common Lisp reader; 本 DSL 是数据语言, 不是 CL 程序")

  (node-types
    (architecture
      :desc "一个程序或系统的总架构"
      :required [ingress logic-core egress]
      :children [pillar interface dependency-map])

    (pillar
      :desc "系统的一级 ownership 单元"
      :required [pillar-ingress pillar-core pillar-egress]
      :children [function capability-family flow section module invariants])

    (capability-family
      :desc "一组对外能力或工具族"
      :required [ingress logic-core egress]
      :children [tool function flow])

    (function
      :desc "pillar 内可独立理解的功能原子/分子"
      :required [ingress logic-core egress]
      :children [step function interface invariants])

    (flow
      :desc "跨 pillar 的 ordered choreography"
      :required [ingress logic-core egress]
      :children [step branch invariant])

    (tool
      :desc "对外 endpoint 原子"
      :required [ingress logic-core egress]
      :children [step validation dispatch audit])

    (step
      :desc "有顺序的最小执行动作"
      :required [:id :owner :action]
      :optional [:reads :writes :emits :returns :calls :guards :errors])

    (handoff
      :desc "执行结果从 operational control-plane 交到 durable artifact-plane 的协议"
      :required [:control-plane :durability-plane :claim-scope :verification :receipt]
      :optional [:commit-policy :rollback :blocker :audit-rule])

    ;; ── v0.2 新增: source-index 节点类型 ──
    (source-index
      :desc "v2 时代 pillar 真实状态索引 — 哪个 lisp 文件代表哪个 pillar/section 的真相"
      :required [:scope]
      :children [pillar-section-index source-of-truth-rule policy-block])

    (pillar-section-index
      :desc "pillar 内可寻址章节集合"
      :required [:pillar :source-file]
      :children [section-entry])

    (section-entry
      :desc "单个可寻址章节, stable id 不随标题文案改名"
      :required [:section-id :title :source-file :status]
      :optional [:local-path :implements :implementation-targets :children
                 :cross-ref :owns-tables :owns-tools :owns-flows :note]))

  (required-shape
    (pillar   [pillar-ingress pillar-core pillar-egress])
    (function [ingress logic-core egress])
    (flow     [ingress logic-core egress])
    (tool     [ingress logic-core egress])
    (source-index [pillar-section-index]))

  ;; ── v0.2 新增: section-id 命名 + 锚点策略 ──
  (section-id-policy
    :goal "压缩/拆分主 Lisp 时不丢 cross-ref"
    :format "kebab-case 全小写; pillar 前缀 + 局部段名, e.g. memory.module.board"
    :stability-rule "section-id 一旦发布即冻结; 即使标题文案修改, section-id 不变"
    :rename-rule "如 section 必须改名, 在 source index 用 :prev-id 列出旧 id, 不删旧条目"
    :no-line-anchor "禁止用具体行号做锚点 — 行号随主文件演进必失效"
    :path-anchor "用 'pillar :: section :: subsection' 这种 local-path 字符串锚定"
    :file-anchor ":source-file 必须是相对路径, 仓库根起算"
    :examples
      ["memory.module.board"
       "memory.module.directive-layer"
       "worker.section.workers"
       "worker.section.claudecode-workstation-orchestration"
       "tools.section.rpc-gateway"
       "intent-layer.section.unified-entry-pipeline"
       "event-bus.section.ingress"
       "event-bus.section.persistence-layer"
       "system-layer.section.rpc-gateway"
       "flow.unified-entry-pipeline"])

  ;; ── v0.2 新增: status taxonomy ──
  (status-taxonomy
    :rationale "判断每个 section 的代码兑现状态, 后续压缩可按状态归并"
    :values
      ((architecture-designed
         :desc "Lisp 已定稿, 代码尚未实现 / 未对齐"
         :compressible "section 正文不动; 只允许压缩重复状态文本")
       (code-aligned
         :desc "Lisp 与代码 1:1 对齐"
         :compressible "可在 status 行批量提取; 主 step/owns/flow 内容保留")
       (code-aligned-partial
         :desc "部分对齐 — 主路径 OK, 边角 / 高级语义仍 pending"
         :compressible "保留 ingress/logic-core/egress; 仅压缩 pending 备注重复块")
       (operational-practice
         :desc "尚未代码化, 但有现网/操作流程层落地"
         :compressible "保留 policy 描述; 压缩多次重复的 'rationale 同上' 段落")
       (pending
         :desc "纯设计意图, 未排期"
         :compressible "可批量短格式")
       (deprecated
         :desc "已弃用, 保留为历史"
         :compressible "可大幅压缩, 仅留指针")
       (protected
         :desc "frozen / architecture-unlocked-but-record-required, 不允许常规 LLM 改动"
         :compressible "禁止压缩正文; 仅允许加 :status / :section-id 元数据")))

  ;; ── v0.2 新增: split-policy ──
  (split-policy
    :goal "决定何时把主 lisp 的某 section 拆成独立 shard 文件"
    :rules
      ((rule-1
         :name "size-threshold"
         :rule "单 section 在主 lisp 中超过 ~400 行连续正文, 候选拆分"
         :exception "frozen / protected 的 section 即使大也不动 (会跨多次 cutover)")
       (rule-2
         :name "ownership-clarity"
         :rule "若 section 已经有专档 (e.g. intent-memory.lisp), 主 lisp 仅保留导航摘要 + section-id 索引")
       (rule-3
         :name "cross-ref-budget"
         :rule "拆分前必须先在 intent-pillar-source-index.lisp 注册 section-id, 不能裸拆")
       (rule-4
         :name "no-circular-shard"
         :rule "shard 之间不互相 :file-ref; 所有 cross 必须经 source index")
       (rule-5
         :name "drift-record"
         :rule "拆分动作必须在对应 pillar 的 *-execution.lisp 留 D-deviation 记录"))
    :wait-for-conditions
      ["file-first .lisp writer (alignment / plan / workflow) 已稳定写出对应 artifact"
       "review-gate 能机读这些 artifact 给出 question"
       "PLAN DAG scheduler 跑过最小闭环, 已观察一次 plan-runner 全程"
       "至少一次 section-id 命名经过 checker --strict 验证"]
    :order-of-operations
      ["先稳定 section-id (本次)"
       "再稳定 file-first writer + review gate + PLAN DAG"
       "再做 status batch 压缩 (按 status-taxonomy 的 compressible 字段)"
       "最后才做物理 split 到 shard"])

  ;; ── v0.2 新增: compression-policy ──
  (compression-policy
    :goal "压缩重复状态文本, 不压缩可执行语义"
    :allowed
      ((compress-status-strings
         :desc "把 :canonical-status / :status 等长描述提取到 source index 的 status block"
         :method "主 lisp 保留短 status 标签, 详细 changelog 落到 execution log")
       (compress-redundant-pointers
         :desc "若多个 section 重复 cross-ref 同一 :target, 提到 source index 的 implementation-targets"
         :method "主 lisp 用 section-id 引用, 不再重复完整路径")
       (compress-changelog-tail
         :desc ":vX.Y-change / :vX.Y-rename / :prior-revisions 已超 5 条的, 压成 'see execution log'"
         :method "主 lisp 只留最新 1 条; 历史在 *-execution.lisp 全保留"))
    :forbidden
      ((dont-compress-ingress-core-egress
         :desc "ingress / logic-core / egress 三段 step 列表是执行约定, 一个字都不删")
       (dont-compress-step-bodies
         :desc "step 的 :reads / :writes / :emits / :returns / :calls 是契约, 全保留")
       (dont-compress-invariants
         :desc "invariant / :guarantee / :stateless 字段是软合约, 不允许压缩")
       (dont-compress-frozen-design
         :desc "file-governance lock=frozen / architecture-unlocked-but-record-required 的文件正文不动")
       (dont-cross-pillar-merge
         :desc "禁止把不同 pillar 的相似 section 合并到一处, 即便文本几乎一样 — pillar boundary 优先"))
    :who-can-compress
      ["主 Claude 在通过 lisp-review skill 后"
       "gptpro 给 governance design diff 后, 由施工会话落笔"]
    :record-required
      ["每次压缩动作在对应 pillar 的 *-execution.lisp 写 D-deviation"
       "压缩前后跑 scripts/check-architecture-lisp.mjs --all-v2 必须 OK"])

  (semantic-rules
    (R001 :name "single-owner"
          :rule "每个 function/flow/tool 的主 owner 必须能追溯到一个 pillar")
    (R002 :name "ordered-steps"
          :rule "logic-core 内 step id 应按 s1/s2/s3 顺序递增")
    (R003 :name "explicit-egress"
          :rule "每个 egress 至少声明 writes / emits / returns / downstream 之一")
    (R004 :name "no-hidden-cross-pillar-call"
          :rule "跨 pillar 调用必须写在 :calls / :to-* / :cross-pillar 中")
    (R005 :name "tool-flow-ref"
          :rule "每个 tool 必须能分类为 named-flow / shared-flow / trivial-single-step / pending-with-reason")
    (R006 :name "no-runtime-in-flow"
          :rule "flow 只写 choreography, runtime mechanics 必须归 worker")
    (R007 :name "no-schema-in-worker"
          :rule "worker 只能读写 memory owned schema, 不能声明 schema ownership")
    ;; ── v0.2 新增 ──
    (R008 :name "section-id-stable"
          :rule "section-id 一旦在 intent-pillar-source-index.lisp 出现即冻结; 改名走 :prev-id, 不删旧 entry")
    (R009 :name "no-line-number-anchor"
          :rule "不允许用 'lineN' / 'line 123' 做 cross-ref; 必须 section-id + local-path")
    (R010 :name "status-must-be-taxonomy"
          :rule "section-entry :status 必须是 status-taxonomy 列出的 7 个值之一; partial / draft 等口语化标签需归并")
    (R011 :name "implements-relative-path"
          :rule ":implements / :implementation-targets 必须是仓库根起的相对路径; 不允许 ~/ 或绝对路径")
    (R012 :name "frozen-file-no-compression"
          :rule "intent-event-bus.lisp 等 frozen / unlocked-record-required 文件不参与压缩, 只能加 :section-id 元数据")
    ;; ── v0.3 新增 ──
    (R013 :name "execution-dual-plane"
          :rule "写文件/代码的 execution 必须同时声明 control-plane(共享 execution Lisp/manager) 与 durability-plane(git commit/patch/artifact), 不允许只写 operational completion")
    (R014 :name "scoped-commit-subset"
          :rule "scoped commit 的 staged_files 必须是 claimed_scope 子集; 越界必须先写 deviation/issue 并重新 claim"))

  (checker-contract
    :phase-1 ["parse all files" "balanced () and []" "unterminated string" "unexpected delimiter"]
    :phase-2 ["recursive contract files must have pillar-ingress/pillar-core/pillar-egress"
              "pillar-core function blocks must have ingress/logic-core/egress"
              "direct step sequence should be s1/s2/s3"]
    :phase-3-precompression
      ["intent-pillar-source-index.lisp 必须 parse"
       "出现的 section-id 全 kebab-case, 不带空格 / 不以数字开头"
       "section-entry :status 落在 status-taxonomy 7 值之内 (后续 checker 升级时启用)"
       "section-entry :source-file 都存在于 .missiond/v2/ 下"
       "frozen 文件清单 (event-bus / unlocked-record-required) 不被改 (git diff --check 留给外部)"]
    :future-phases ["load this defdsl as data"
                    "validate required-shape dynamically"
                    "emit JSON IR"
                    "generate Mermaid / Markdown / review checklist"])

  ;; ── v0.2 新增: 当前主决策记录 ──
  (judgement-now
    :date "2026-04-26"
    :decided-by "wave 11 lisp-source-index-precompression session"
    :decisions
      ((d1 :name "no-main-lisp-compression-yet"
           :reason "在 file-first writer / review gate / PLAN DAG 最小闭环稳定前, 压缩主 Lisp 会让其他并行会话失锚")
       (d2 :name "build-index-and-checker-first"
           :reason "section-id / status taxonomy / split rule 是压缩的先决条件, 必须先冻结")
       (d3 :name "wait-for-three-checkpoints-before-compression"
           :checkpoints
             ["file-first writer (alignment.lisp / PLAN.lisp / workflow.lisp) 自动写入 stable"
              "review gate 能基于 artifact 自动出 QuestionEvent"
              "PLAN DAG scheduler 跑过最小闭环 + ExecutionEvent dispatch metadata code-aligned"]
           :then "再回头按 compression-policy 批量压缩状态文本; 最后才物理 split shard"))))
