;; ═════════════════════════════════════════════════════════════
;; MissionD — Reusable Architecture DSL (architecture-v1)
;; 目标: 把"pillar / function / flow 都有 ingress → logic-core → egress"
;;       升格为可复用、可检查、可生成图的 Lisp DSL。
;; 适用: MissionD v2 架构 Lisp, 以及未来其他程序的架构 Lisp。
;; ═════════════════════════════════════════════════════════════

(defdsl architecture-v1
  :version "v0.6"
  :status "declarative schema 2026-04-26 — reader/checker first; v0.2 adds source-index taxonomy (precompression); v0.3 adds execution handoff dual-plane rule; v0.4 (wave 12 task 06) adds source-index entry mandatory-fields rule + section-id uniqueness rule + :compression-safe? optional field; v0.5 (wave 14 task 07) adds l2-shard-split-plan (5 candidate shards designed, NOT executed; execution-gate + per-shard moved-sections / retained-anchor / source-index-update-rule / checker-requirement / rollback-plan); v0.6 (wave 15 task 03) adds R017 source-file-must-exist + R018 source-file-must-live-under-v2 + shard auto-discovery via collectSourceFileRefs (data-driven, no hardcoded shard paths)"
  :checker "scripts/check-architecture-lisp.mjs"

  (purpose
    "声明架构 Lisp 的通用语法骨架; 第一阶段只做 parse + shape validation, 不做代码生成"
    "v0.2 扩: 在 主 Lisp 真正压缩/拆分前, 先固定 section-id / status / split / compression 约定,"
    "        让 pillar source index 可机器读, 后续压缩不丢 cross-ref"
    "v0.3 扩: 执行型 flow 可声明 control-plane / durability-plane handoff, 防止 operational report 与代码成果脱节"
    "v0.4 扩: source-index entry 强制三必填 (file / local-path / status), section-id 全局唯一; 加 :compression-safe? 字段, 让未来批量压缩可白名单驱动"
    "v0.5 扩: 加 l2-shard-split-plan, 设计 5 候选 shard (intent-execution-governance / intent-directive-artifacts / intent-plan-dag / intent-capability-governance / intent-workstation-policy); 每 shard 含 moved-sections / retained-anchor / source-index-update-rule / checker-requirement / rollback-plan; execution-gate 全满足才允许实际拆分; 本 v0.5 不移动任何 shard 内容"
    "v0.6 扩: shard-aware checker — R017 (source-file 必存在) + R018 (source-file 必 .missiond/v2/) + checker 通过 source-index 自动发现 shard 文件 (data-driven, 不 hardcode); 报告行显示 initial + auto-discovered shard 数, 方便 review 验证 wave 15 后 5 shard 实际接入")

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
                 :cross-ref :owns-tables :owns-tools :owns-flows :note
                 :compression-safe? :prev-id :pillar-section-index-ref]))

  ;; ── v0.4 新增: source-index entry 扩展约定 (wave 12 task 06) ──
  (section-entry-extended
    :desc "对 section-entry 节点的扩展规则 — 让 wave 12 起的所有新 entry 满足 file/local-path/status 必填 + section-id 全局唯一 + :compression-safe? 可选"
    :mandatory-fields-on-write [:section-id :source-file :local-path :status]
    :uniqueness-scope "intent-pillar-source-index.lisp 全文; section-id 全局唯一, 与 pillar 无关"
    :compression-safe-field
      ((true   :meaning "section 正文允许走 compression-policy.allowed 中三类压缩; 仍受 forbidden 红线约束")
       (false  :meaning "section 正文是契约 / control-plane / durability-plane / frozen, 即使 compression-policy 允许也不动")
       (absent :meaning "缺省视为 unknown, 默认按 false 处理 — 必须先显式标 true 才允许压缩"))
    :rename-policy "改名走 :prev-id 字段, 旧 entry 不删 — 与 R008 保持一致"
    :back-compat "v0.2 的 baseline 7 pillar entry 没有 :compression-safe?, 后续可 lazy 补; 不视为 lint 阻断"
    :checker-status "phase-3.1-precompression-coverage IMPLEMENTED in scripts/check-architecture-lisp.mjs (wave 14 task 05) — R015 mandatory-fields + R016 section-id-uniqueness + :compression-safe? value enum (true|false|yes|no|safe|unsafe|defer) enforced on intent-pillar-source-index.lisp under --all-v2; :local-path prefix soft rule still warn-only deferred")

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
          :rule "scoped commit 的 staged_files 必须是 claimed_scope 子集; 越界必须先写 deviation/issue 并重新 claim")
    ;; ── v0.4 新增 (wave 12 task 06) ──
    (R015 :name "source-index-entry-mandatory-fields"
          :rule "intent-pillar-source-index.lisp 内每个 section-entry 必须三必填: :source-file (file) + :local-path + :status; 缺任一字段 checker phase-3.1 报 missing-required-field")
    (R016 :name "section-id-global-unique"
          :rule "intent-pillar-source-index.lisp 内 :section-id 全局唯一; 重名 checker phase-3.1 报 duplicate-section-id; 改名走 :prev-id, 不删旧 entry")
    ;; ── v0.6 新增 (wave 15 task 03) — shard-aware source-file 校验 ──
    (R017 :name "source-file-must-exist"
          :rule "intent-pillar-source-index.lisp 内 (pillar-section-index :source-file ...) 与 (section-entry :source-file ...) 引用的路径必须真实存在; 缺失 checker phase-3.2 报 source-file-does-not-exist; 防止 L2 shard rename / move 后留下死链")
    (R018 :name "source-file-must-live-under-v2"
          :rule "intent-pillar-source-index.lisp 内 :source-file 必须以 '.missiond/v2/' 起头; 跨目录引用先归并到 v2/ 下的 shard, 再在 source-index 注册; 防止 source-index 退化为通用文件清单"))

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
    ;; ── v0.4 新增 (wave 12 task 06) — checker future rule ──
    :phase-3.1-precompression-coverage
      ["每个 section-entry 必填三字段: :source-file (file) + :local-path + :status (R015)"
       ":section-id 全局唯一 — 在 intent-pillar-source-index.lisp 全文聚合, 重名报 duplicate-section-id (R016)"
       ":compression-safe? 字段若出现, 值必须是 true / false 之一; 缺省按 false 处理 (无强制要求)"
       ":local-path 字符串必须以 'pillar ' 或 'defdsl ' 开头, 表示从 root context 起算的语义路径 (软规则, warn-only)"
       ":implements 路径都是仓库根起算相对路径 (R011 复用)"]
    :phase-3.1-status "IMPLEMENTED in scripts/check-architecture-lisp.mjs (wave 14 task 05) — R015 mandatory-fields + R016 section-id-uniqueness enforced; :compression-safe? value enum widened to true|false|yes|no|safe|unsafe|defer; :local-path prefix soft rule deferred"
    ;; ── v0.6 新增 (wave 15 task 03) — shard-aware checker ──
    :phase-3.2-shard-aware
      ["每个 :source-file 路径必须真实存在 (R017) — 防止 L2 shard rename / move 留下死链"
       "每个 :source-file 必须以 '.missiond/v2/' 起头 (R018) — source-index 不退化为跨目录通用清单"
       "checker 自动从 intent-pillar-source-index.lisp 内 :source-file 引用集合反向拉入待检查 shard 文件 — 不在 checker 源码中 hardcode shard 路径"
       "当只传 intent-pillar-source-index.lisp 给 checker 时, 自动 union shard 文件; --all-v2 仍兜底全量"
       "report 行明确显示初始文件数 + 自动发现 shard 数, 让 reviewer 能验证 wave 15 后多了几张 shard"]
    :phase-3.2-status "IMPLEMENTED in scripts/check-architecture-lisp.mjs (wave 15 task 03) — R017 source-file-must-exist + R018 source-file-must-live-under-v2 enforced on (pillar-section-index :source-file ...) and (section-entry :source-file ...); shard auto-discovery via collectSourceFileRefs; --dry-fixture 扩 10 fixture (含 R017 missing / R018 outside / R018 短路 / pillar-section-index header / shard auto-discovery)"
    :future-phases ["load this defdsl as data"
                    "validate required-shape dynamically"
                    "emit JSON IR"
                    "generate Mermaid / Markdown / review checklist"])

  ;; ── v0.5 新增 (wave 14 task 07): L2 shard split plan ──
  ;; 设计 5 候选 shard, 但本 wave 不移动任何 shard 内容; 真正拆分由后续 wave 在 execution-gate 全满足后做
  (l2-shard-split-plan
    :version "v0.5"
    :date "2026-04-26"
    :decided-by "wave 14 / task 07 lisp backfill + L2 shard split plan session"
    :goal "为 5 个高变动语义区设计独立 shard 文件, 让主 Lisp (intent-flow.lisp / intent-intent-layer.lisp / intent-memory.lisp / intent-tools.lisp / intent-worker.lisp) 缩瘦, 同时保持 cross-ref 不失锚"
    :scope-non-goal
      ["本 v0.5 plan 不移动任何 shard 内容"
       "本 v0.5 plan 不增减 section-id"
       "本 v0.5 plan 不修改主 Lisp 大段正文"
       "L2 实际执行 (拆 shard) 必须等 execution-gate 5 项全满足"]
    :ordering-rule "L2 实际执行需按 split-policy.order-of-operations: section-id 已稳定 (✓) → file-first writer + review gate + PLAN DAG 稳定 (✓ wave 14 task 01/02/03) → status batch 压缩 (部分 wave 13 task 05 完成) → 物理 split (本 plan 等 gate)"

    ;; ── execution gate (全部满足才允许拆) ──
    (execution-gate
      :description "L2 实际拆分前必须四项全满足"
      :gates
        ((gate-1 :name "source-index-checker-passing"
                 :requirement "scripts/check-architecture-lisp.mjs --all-v2 OK + R015 + R016 enforced"
                 :status "satisfied (wave 14 task 05 commit 5c60f82 — checker IMPLEMENTED, --all-v2 14 files OK, --dry-fixture 5 fixtures PASS)"
                 :anchor "intent-pillar-source-index.lisp :: intent-layer.source-index-checker.r015-r016-implemented")
         (gate-2 :name "file-first-writer-stable"
                 :requirement "directive / plan / workflow 三类 artifact 全走统一 helper, partial 语义清晰, 无 process cwd fallback"
                 :status "satisfied (wave 14 task 01 commit 00cbc1d — 主路径接入, 6 file_* 响应字段, dead_code 全清)"
                 :anchor "intent-pillar-source-index.lisp :: memory.directive-layer.file-first-writer-integration")
         (gate-3 :name "review-gate-stable"
                 :requirement "review_gate policy enum (manual|emit_question|off) auto-create, deterministic id, default byte-identical"
                 :status "satisfied (wave 14 task 03 commit 96842cd — policy enum + 自动 emit + deterministic id)"
                 :anchor "intent-pillar-source-index.lisp :: intent-layer.unified-entry-pipeline.review-gate-policy")
         (gate-4 :name "no-active-parallel-code-wave"
                 :requirement "无 wave 14 平行写代码会话仍在改 ownership 文件 (intent-flow.lisp / intent-intent-layer.lisp / intent-memory.lisp / intent-tools.lisp / intent-worker.lisp)"
                 :status "to-be-verified (本 wave 14 task 07 是最后一个 task; wave 14 task 00-06 全部 commit; wave 15 平行 wave 必须排除 ownership 重叠)"
                 :anchor "git log + .missiond/claudecode/wave14-* untracked check"))
      :all-satisfied? "3 of 4 satisfied; gate-4 须 wave 15 starting 时确认"
      :rollback-rule "若 L2 拆分后任一新 shard 引入 cross-ref 失锚 / checker 报 error / parallel wave 冲突, 按本 plan :rollback-plan 字段回滚")

    ;; ── 5 candidate shards (per-shard spec) ──
    (candidate-shards
      :description "5 候选 shard, 每 shard 含 moved-sections / retained-anchor / source-index-update-rule / checker-requirement / rollback-plan"

      (shard intent-execution-governance
        :proposed-path ".missiond/v2/intent-execution-governance.lisp"
        :rationale "执行治理 (mission_execution 12-action manager + scoped commit handoff + companion log meta) 现散在 intent-flow.lisp + intent-tools.lisp + intent-intent-layer.lisp 三处; 提到独立 shard 后单一 owner"
        :moved-sections
          ["intent-flow.lisp :: F-execution-log-governance"
           "intent-flow.lisp :: F-scoped-commit-handoff"
           "intent-tools.lisp :: implemented-surface mission_execution"
           "intent-intent-layer.lisp :: section unified-entry-pipeline :: role plan-runner :: execution-substrate sub-block"]
        :retained-anchor-in-parent
          ["intent-flow.lisp 留 stub (flow F-execution-log-governance :file-ref 'intent-execution-governance.lisp')"
           "intent-flow.lisp 留 stub (flow F-scoped-commit-handoff :file-ref ...)"
           "intent-tools.lisp 留 stub (implemented-surface mission_execution :file-ref ...)"
           "intent-intent-layer.lisp plan-runner role 保持完整, execution-substrate 只留 cross-ref 到本 shard"]
        :source-index-update-rule
          ["在 intent-pillar-source-index.lisp 加 (pillar-section-index :pillar execution-governance :source-file '.missiond/v2/intent-execution-governance.lisp')"
           "重定向 flow.execution-log-governance / flow.scoped-commit-handoff / tools.surface.mission_execution 等 entry 的 :source-file"
           "保持 :section-id 不变 (R008); 移动只换 :source-file 与 :local-path"]
        :checker-requirement
          ["scripts/check-architecture-lisp.mjs --all-v2 必须 OK"
           "section-id 全部出现在 source-index, R015 + R016 满足"
           "新 shard 必须含 (recursive-architecture-contract ...) 或明确声明 sub-shard"
           "git diff --check 通过"]
        :rollback-plan
          ["若 checker 报 error: git revert 该拆分 commit; source-index :source-file 回退到原文件"
           "若 cross-ref 失锚: 用 :prev-id 在 source-index 注册旧 path → 新 shard path 映射, 等 cross-ref 消费方更新"
           "若 parallel code wave 冲突: 暂停拆分, 等 parallel wave 完成 commit 再 rebase"]
        :estimated-line-impact "约 600-800 行 → 移到 shard; 主文件减 ~700 行"
        :status "designed (not executed)")

      (shard intent-directive-artifacts
        :proposed-path ".missiond/v2/intent-directive-artifacts.lisp"
        :rationale "directive-layer 三类 artifact (file-first-artifacts / file-first-writer-integration / 5 artifact registry) + review_gate policy 集中于 directive/plan/workflow handler; 提到独立 shard 后 schema 与 writer integration 单一 owner"
        :moved-sections
          ["intent-memory.lisp :: module directive-layer :: file-first-artifacts (5 artifact 详细字段)"
           "intent-memory.lisp :: module directive-layer :: file-first-writer-integration"
           "intent-intent-layer.lisp :: section unified-entry-pipeline :: review-gate-policy + review-gate-id-derivation"
           "intent-tools.lisp :: tools.surface.directive-write-file-args / plan-write-file-args / workflow-write-file-args / review-gate-args"]
        :retained-anchor-in-parent
          ["intent-memory.lisp 留 stub (module directive-layer :file-ref 'intent-directive-artifacts.lisp' + status-summary cross-ref)"
           "intent-intent-layer.lisp 留 stub (section unified-entry-pipeline :: review-gate :file-ref ...)"
           "intent-tools.lisp 留 stub (implemented-surface mission_directive/plan/workflow :: write_file/review_gate args :file-ref ...)"]
        :source-index-update-rule
          ["在 intent-pillar-source-index.lisp 加 (pillar-section-index :pillar directive-artifacts :source-file '.missiond/v2/intent-directive-artifacts.lisp')"
           "memory.directive-layer.* 与 intent-layer.unified-entry-pipeline.review-gate-* entry 的 :source-file 重定向"
           ":section-id 不改, 只换 :source-file"]
        :checker-requirement
          ["scripts/check-architecture-lisp.mjs --all-v2 必须 OK"
           "新 shard parse OK"
           "review_gate policy enum 三态 完整保留"
           "git diff --check 通过"]
        :rollback-plan
          ["若 review_gate 行为退化: revert 拆分 commit; review_gate.rs 不动, 仅 Lisp 回退"
           "若 cross-ref 失锚: source-index :prev-id 注册旧 path 映射"]
        :estimated-line-impact "约 500-700 行 → 移到 shard; 主文件减 ~600 行"
        :status "designed (not executed)")

      (shard intent-plan-dag
        :proposed-path ".missiond/v2/intent-plan-dag.lisp"
        :rationale "PLAN DAG scheduler (actor + runtime v2 + node lifecycle + failure-policy + node schema + FSM + open-questions) 是 intent-intent-layer.lisp 最大 section, ~400+ 行 高变动语义; 提到独立 shard 后 v1 完整 11-stage 实现可独立迭代"
        :moved-sections
          ["intent-intent-layer.lisp :: section action-instruction-actor :: actor plan-dag-scheduler (完整 11-stage / claim-lease / retry / rollback / acceptance / review-gate paused / mark-plan-final / trigger-record-execution-distill)"
           "intent-intent-layer.lisp :: section action-instruction-actor :: actor plan-dag-scheduler :: node schema + node FSM + open-questions"
           "intent-flow.lisp :: F-intent-alignment-plan-execution-loop :: s6 execution-runner :: dag-scheduler sub-block (完整 11-stage 协议)"
           "intent-memory.lisp :: module directive-layer :: artifact plan-node-state-projection (per-node 字段 nodes[] 子树)"]
        :retained-anchor-in-parent
          ["intent-intent-layer.lisp 留 stub (section action-instruction-actor :: actor plan-dag-scheduler :file-ref 'intent-plan-dag.lisp' + 当前 runtime v2 status)"
           "intent-flow.lisp 留 stub (s6 execution-runner :: dag-scheduler :file-ref ... + runtime v2 status)"
           "intent-memory.lisp 留 stub (artifact plan-node-state-projection :file-ref ... + 顶层字段 status)"]
        :source-index-update-rule
          ["在 intent-pillar-source-index.lisp 加 (pillar-section-index :pillar plan-dag :source-file '.missiond/v2/intent-plan-dag.lisp')"
           "intent-layer.plan-dag-runtime-v2.* / intent-layer.actor.plan-dag-scheduler / flow.execution-runner-dag-scheduler / memory.directive-layer.plan-node-state-projection / event-bus.section.execution-event.plan-node-state-changed entry 的 :source-file 重定向"
           ":section-id 不改, 只换 :source-file"]
        :checker-requirement
          ["scripts/check-architecture-lisp.mjs --all-v2 必须 OK"
           "新 shard 含 actor / FSM / open-questions 完整结构"
           "PlanNodeStateChanged variant 仍 protected via event-bus.lisp 不进 shard"
           "git diff --check 通过"]
        :rollback-plan
          ["若 11-stage actor 设计与 runtime v2 实现冲突: revert + 在 shard 内独立标 architecture-designed pending, 不动 plan_dag.rs 代码"
           "若 cross-ref 失锚: :prev-id 注册"]
        :estimated-line-impact "约 700-900 行 → 移到 shard; 主文件减 ~800 行"
        :status "designed (not executed)")

      (shard intent-capability-governance
        :proposed-path ".missiond/v2/intent-capability-governance.lisp"
        :rationale "capability-evolution-governance (semantic evidence v1 + DispatcherEvent / WorkerEvent 聚合 + workflow stats + capability_usage tool / read-model) 集中于 capability_usage handler 周边; 提到独立 shard 后语义合并决策可独立迭代"
        :moved-sections
          ["intent-intent-layer.lisp :: section capability-evolution-governance (5 sources + lisp hint merge-candidate)"
           "intent-flow.lisp :: F-capability-usage-monitoring (5 sources monitoring narrative)"
           "intent-tools.lisp :: implemented-surface mission_capability_usage (action/window/scope/replacement_target/dry_run schema)"
           "intent-memory.lisp :: module system-support :: derived-read-model capability-usage-read-model"]
        :retained-anchor-in-parent
          ["intent-intent-layer.lisp 留 stub (section capability-evolution-governance :file-ref ...)"
           "intent-flow.lisp 留 stub (F-capability-usage-monitoring :file-ref ...)"
           "intent-tools.lisp 留 stub (implemented-surface mission_capability_usage :file-ref ...)"
           "intent-memory.lisp 留 stub (derived-read-model :file-ref ...)"]
        :source-index-update-rule
          ["在 intent-pillar-source-index.lisp 加 (pillar-section-index :pillar capability-governance :source-file '.missiond/v2/intent-capability-governance.lisp')"
           "intent-layer.capability-evolution-governance.* / flow.capability-usage-monitoring / tools.surface.mission-capability-usage / memory.system-support.capability-usage-read-model entry 的 :source-file 重定向"
           ":section-id 不改, 只换 :source-file"]
        :checker-requirement
          ["scripts/check-architecture-lisp.mjs --all-v2 必须 OK"
           "5 sources 列表完整保留 (契约段不压缩)"
           "schema 字段 (action/window/scope/replacement_target/dry_run) 完整保留"
           "git diff --check 通过"]
        :rollback-plan
          ["若 capability_usage handler 与 shard 设计冲突: revert + 标 capability_usage 实现仍 code-aligned partial"
           "若 cross-ref 失锚: :prev-id 注册"]
        :estimated-line-impact "约 400-500 行 → 移到 shard; 主文件减 ~450 行"
        :status "designed (not executed)")

      (shard intent-workstation-policy
        :proposed-path ".missiond/v2/intent-workstation-policy.lisp"
        :rationale "ClaudeCode workstation orchestration policy (resident-lisp / fresh-code-alignment / agent-team / mixed / prompt-fallback + dispatch-decision-matrix + execution-strategy-record + spawn-over-prompt-mode + project-root-cwd + scoped-commit-handoff) 跨 worker / flow / intent-layer 三 pillar; 提到独立 shard 后单一 owner"
        :moved-sections
          ["intent-worker.lisp :: section claudecode-workstation-orchestration (含 dispatch-decision-matrix + execution-strategy-record + 6 个 principle)"
           "intent-flow.lisp :: F-workstation-dispatch-policy"
           "intent-intent-layer.lisp :: section unified-entry-pipeline :: workstation-dispatch-policy (含 4 个 principle)"]
        :retained-anchor-in-parent
          ["intent-worker.lisp 留 stub (section claudecode-workstation-orchestration :file-ref ...)"
           "intent-flow.lisp 留 stub (F-workstation-dispatch-policy :file-ref ...)"
           "intent-intent-layer.lisp 留 stub (workstation-dispatch-policy :file-ref ...)"]
        :source-index-update-rule
          ["在 intent-pillar-source-index.lisp 加 (pillar-section-index :pillar workstation-policy :source-file '.missiond/v2/intent-workstation-policy.lisp')"
           "worker.section.claudecode-workstation-orchestration.* / flow.workstation-dispatch-policy / intent-layer.unified-entry-pipeline.workstation-dispatch-policy entry 的 :source-file 重定向"
           ":section-id 不改, 只换 :source-file"]
        :checker-requirement
          ["scripts/check-architecture-lisp.mjs --all-v2 必须 OK"
           "dispatch-decision-matrix 决策表完整保留 (契约段不压缩)"
           "execution-strategy-record 字段 (dispatch_strategy / target_project / requested_cwd) 完整保留"
           "git diff --check 通过"]
        :rollback-plan
          ["若 workstation orchestration 与 plan_dag dispatch 实现冲突: revert + 标 workstation policy 仍 operational-practice"
           "若 cross-ref 失锚: :prev-id 注册"]
        :estimated-line-impact "约 300-400 行 → 移到 shard; 主文件减 ~350 行"
        :status "designed (not executed)"))

    ;; ── 总体 invariants ──
    (cross-cutting-invariants
      :description "L2 实际执行任意 shard 时必须遵守的全局约束"
      :rules
        [(rule-1 :name "section-id-stability"
                 :rule "拆分前后 :section-id 不改 (R008); 改名走 :prev-id, 不删旧 entry")
         (rule-2 :name "no-circular-shard-ref"
                 :rule "shard 之间不互相 :file-ref; 所有 cross 必须经 source-index (split-policy.rule-4)")
         (rule-3 :name "checker-must-pass-after-each-shard"
                 :rule "每拆一个 shard 提交一次 commit, checker --all-v2 必须 OK; 不允许多 shard 一起拆")
         (rule-4 :name "drift-record"
                 :rule "拆分动作必须在对应 pillar 的 *-execution.lisp 留 D-deviation 记录 (split-policy.rule-5)")
         (rule-5 :name "no-content-mutation"
                 :rule "拆分时严禁压缩或 reword section 正文; shard 内容必须与原主 Lisp byte-identical (压缩另起 commit)")
         (rule-6 :name "frozen-files-not-shardable"
                 :rule "intent-event-bus.lisp / intent-event-bus-execution.lisp / intent-mcp-defs.lisp 已锁, 不参与拆分")])

    ;; ── 估算 ──
    (estimated-impact
      :total-line-reduction-in-main-files "~3000 行 (5 shard 各 ~600 行)"
      :main-files-after-split
        ["intent-flow.lisp 减 ~1500 行 (execution-governance + plan-dag + capability-governance + workstation-policy 各搬出一段)"
         "intent-intent-layer.lisp 减 ~1300 行 (directive-artifacts review-gate + plan-dag actor + capability-governance + workstation-policy)"
         "intent-memory.lisp 减 ~700 行 (directive-artifacts + capability-usage-read-model + plan-node-state-projection)"
         "intent-tools.lisp 减 ~600 行 (mission_execution + mission_directive/plan/workflow write_file/review_gate args + mission_capability_usage)"
         "intent-worker.lisp 减 ~350 行 (claudecode-workstation-orchestration)"]
      :new-shard-files-count 5
      :total-cross-ref-updates "~80 entry :source-file 重定向 (源 index 内, section-id 不变)"))

  ;; ── v0.2 新增: 当前主决策记录 ──
  (judgement-now
    :date "2026-04-26"
    :decided-by "wave 11 lisp-source-index-precompression + wave 12 task 06 source-index-expansion + wave 14 task 07 lisp-backfill-and-l2-shard-plan sessions"
    :wave14-task-07-non-goal
      ["本任务不真正压缩主 Lisp"
       "本任务不实际拆 shard (L2 split plan 仅设计 5 候选, 实际执行延后到 execution-gate 全满足)"
       "本任务只在 v0.4 baseline 上回填 wave 14 真实状态 + 加 l2-shard-split-plan"]
    :decisions
      ((d1 :name "no-main-lisp-compression-yet"
           :reason "L1 安全压缩 wave 13 task 05 已部分完成; L2 物理拆分等四 gate 全满足 (本 v0.5 写 plan, 不执行)")
       (d2 :name "build-index-and-checker-first"
           :reason "section-id / status taxonomy / split rule / shard plan 是压缩与拆分的先决条件, 必须先冻结")
       (d3 :name "wait-for-three-checkpoints-before-compression"
           :checkpoints
             ["file-first writer (alignment.lisp / PLAN.lisp / workflow.lisp) 自动写入 stable — wave 14 task 01 satisfied"
              "review gate 能基于 artifact 自动出 QuestionEvent — wave 14 task 03 satisfied"
              "PLAN DAG scheduler 跑过最小闭环 + ExecutionEvent dispatch metadata code-aligned — wave 14 task 02 部分 satisfied (PlanNodeStateChanged variant + live ref); 完整 11-stage scheduler 仍 architecture-designed pending"]
           :then "再回头按 compression-policy 批量压缩状态文本; 最后按 l2-shard-split-plan 物理 split shard")
       (d4 :name "compression-safe-flag-required-for-batch-compression"
           :reason "section-entry-extended 引入 :compression-safe? 字段; 未来批量压缩必须以该字段做白名单, 缺省按 false 处理 (wave 12 task 06)")
       (d5 :name "l2-shard-split-plan-designed-not-executed"
           :reason "wave 14 task 07 写 5 候选 shard plan + 4-gate execution gate; 本 wave 不移动任何 shard 内容; L2 实际执行需 gate 全满足且无 parallel code wave (wave 14 task 07)"))))
