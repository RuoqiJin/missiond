;; MissionD — Workflow: Pillar 级 Store Manager 重构 (Pillar Refactor 范式)
;; Captured: 2026-04-20
;; Concrete instance: memory pillar refactor (v0.4.16 → v0.5.0 L1 polish)
;;   commit range: fad2850 → c2f01f9
;;   跨 ~1 主会话 (2026-04-19 起多日多轮)
;;
;; 关系: 和 bus-refactor.lisp (大型基础设施重构) 互补
;;   bus-refactor: 推倒重来的神经中枢级 (e.g. event-bus / IPC)
;;   pillar-refactor: 一个 pillar 的完整 store-manager 重构 + lisp↔code 双向同构
;;
;; 用途: 未来对其他 pillar (compute / intent-layer / flow / system) 做
;;   "数据层向 lisp 对齐" 级别重构时, 参考本 workflow 避免重复踩坑.

(workflow pillar-refactor
  :schema "missiond.workflow.methodology.v1"
  :workflow_id pillar-refactor
  :status historical-methodology
  :source_plans [memory-pillar-v0.5.0-refactor]
  :steps [s1 s2 s3 s4 s5]
  :risk-gates [manual-review-only no-runtime-autonomous-execution]
  :completion (:checks ["methodology reference only; not a live execution contract"]
               :artifact "historical pillar refactor workflow")
  (granularity meta-methodology)
  (reusable-for "一个 pillar 的完整 store-manager 重构 (lisp 设计 + execution 施工 + 双向同构验证 + drop migration + polish)")
  (not-for "单 module 小改 / 单 trait 重构 / 纯 feature add / 局部 bug fix")
  (reference-instance "memory pillar v0.4.16 → v0.5.0 @ c2f01f9")

  ;; ═══════════════════════════════════════════════════
  ;; 元模式: 5 大阶段
  ;; ═══════════════════════════════════════════════════
  (meta-phases 5

    (phase-A lisp-design-iteration
      :goal       "pillar lisp 多轮演进 — 从初稿到 frozen 基线"
      :output     "intent-<pillar>.lisp frozen (file-to-module-mapping 骨架 + pillar-interfaces + 9 module × 5 surface 矩阵)"
      :pattern    "多次 unfreeze/refreeze (e.g. v0.4.16→v0.4.17→...→v0.4.23), 每版解决 1-2 个具体问题 + 单独 commit"
      :typical-cost "30-40% 总时间"
      :key-artifacts
        "legacy 表清算 (module ownership 校正)"
        "新 module 设计 (schema-ready-pending-implementation 允许)"
        "pillar-interfaces (正交维度 — mcp / worker-trait / frontend / cross-pillar / external-filesystem)"
        "命名去歧义 (e.g. intent 表 → directive, 避和文件混淆)"
        "野生逻辑清扫 (声明矛盾 + 命名陈旧 + 未归载体)"
        "target-code-layout (施工范围 + in-scope / out-of-scope)")

    (phase-B execution-log-init
      :goal       "开 execution lisp 共享内存层 + agent 扫 in-scope 代码对比"
      :output     "execution lisp (6 slots: phase-tracker/claims/deviations/decisions/completions/issues) + phase-1 scan 结果"
      :typical-cost "5-10% 总时间"
      :pattern    "派 Explore agent 扫骨架列的文件, 对比 lisp → 发现 mismatch → 记 D<NNN>"
      :critical   "agent 遇 lisp 前提错 → STOP 报主会话记 D, 不硬推任务 prompt")

    (phase-C phased-code-alignment
      :goal       "按 6 sub-stage 分批改代码使其向 lisp 对齐"
      :output     "代码大量 -LOC + trait 合并/拆分/新建 + 每 stage 独立 commit"
      :typical-cost "40-50% 总时间"
      :sub-stages
        (stage-2A 热身     "低风险独立动作 — 删冗余 gen_*.rs / rename / 删 dead 空壳")
        (stage-2B 新建壳   "新 trait 骨架先空后填 (如 InfraStore) + 完整 trait 新建 (如 DirectiveLayerStore 17 方法)")
        (stage-2C 合并     "sub-trait → primary 合并, Rust coherence 约束下方法搬到一个 .rs 文件 (如 pg/skill.rs 承载 ProjectStore 33 方法)")
        (stage-2D 拆分     "primary trait 内方法按业务语义拆到新 trait (如 ObservabilityStore 拆出 InfraStore)")
        (stage-2E 大扫除   "legacy 生态一次性清 (如 sqlite/ 整目录 -14018 LOC)")
        (stage-2F 文档     "file-to-module-mapping-complete 补到 execution lisp + 所有 wild files 分类")
      :stage-size "每 stage 可大可小, 取决于 agent 单次可完成性 + cargo build 验证门槛")

    (phase-D bi-directional-validation
      :goal       "lisp ↔ code 双向同构校验 + drop migration + zombie cleanup"
      :output     "audit 报告 + drop migration SQL + D-series doc drift 记录"
      :typical-cost "10-15% 总时间"
      :checks
        "binds-to: 每 writer/reader 的 :cross-ref 对应代码真实存在"
        "lisp→code orphan: lisp 每个声明在代码找得到"
        "code→lisp stray: 代码 pub trait/struct 在 lisp 也被声明"
        "drop migration: 按 frozen lisp 标 DROP-CANDIDATE 的表执行"
        "zombie cleanup: 代码里没人调但 trait 方法还在的 → 删")

    (phase-E post-construction-polish
      :goal       "施工后微调 — 补漏 + 架构升级 + 语义压缩"
      :output     "小版本 bump + L1 语义压缩 (可选)"
      :typical-cost "10-15% 总时间"
      :activities
        "施工发现 lisp 不足 → unfreeze 补漏 (e.g. v0.4.24 补 13 处 doc drift)"
        "新业务需求 → 架构升级 (e.g. v0.5.0 memory-hook 迁 board_tasks)"
        "L1 语义压缩 — history 外移 / table-catalog SSOT / 重复 pattern 合并"))


  ;; ═══════════════════════════════════════════════════
  ;; 核心原则 (从实战提炼)
  ;; ═══════════════════════════════════════════════════
  (principles

    (principle-1 代码向-lisp-对齐
      :statement "lisp 是真相. 代码偏离 lisp 时, 改代码, 不改 lisp (除非发现 lisp 前提错 → 走 deviation 流程)"
      :indicator "指挥官授权 unfreeze 才改 lisp; 日常施工改代码"
      :anti-example "agent 看代码现状先, 发现 lisp 说的不对就擅自改 lisp (禁)")

    (principle-2 稳扎稳打不出错就是快
      :statement "老代码耦合严重 + 新 lisp 架构优雅时, 分 stage 分批 + 每步 cargo build 验证 >> 一次性大 refactor"
      :rationale "回滚成本低 + 每 stage 暴露新认知 (常修正前 stage 假设)"
      :indicator "每 stage 独立 commit, 可 git reset 单步回滚")

    (principle-3 派-agent-team-保护主会话-context
      :statement "调研 + 代码 edit + cargo build 都外包 agent. 主会话只做: orchestrate / 决策 / lisp edit / commit 管理 / 记 execution lisp"
      :payoff "主会话 context 稳定 ~10K tokens, 可跑多 stage 不 OOM")

    (principle-4 deviation-优先记录-不硬改-frozen-lisp
      :statement "agent 遇 lisp 前提错 → STOP, 记 D<NNN> in execution lisp, 等指挥官批 → 决策后执行"
      :examples
        "D001 (db/project.rs 不存在) — agent 误判, 主 Claude ls 验证后决策选项 c (补建)"
        "D003 (TimelineStore 归属) — agent 险些错删, 实际是 pillar 四 projection 读接口"
        "D009 (tasks 表 30+ caller) — 25 个在 pillar 二, 改方案全迁为 phased B"
        "D010 (同 D009 再次 phased agent 又遇到) — 固化决策")

    (principle-5 grep-ability-高于-dsl-紧凑度
      :statement "lisp 压缩时保留 :binds-to / :library-pov 等明显标签, 不激进 DSL 化"
      :rationale "施工 agent 靠 grep 定位契约; macro 化后每次要脑内展开 + LLM context 消耗"
      :applies-to "L1 可做 (history 外移 / SSOT 合并); L2 慎做 (默认化); L3 forge 配套才能做")

    (principle-6 rust-analyzer-不是真相-cargo-是
      :statement "rust-analyzer 因 feature cfg 检测 + worktrees 缓存 常报假 E0046/E0119/E0599. cargo build --workspace 通过 = ground truth"
      :lesson "施工 agent 自诉 '通过' 还要主 Claude cargo 验证 1 次 (多次发现 agent 乐观错报)")

    (principle-7 数据先于-module
      :statement "memory pillar support module (llm-support/slot-support/system-support/embedding-support) 都是'数据已存在 → 后归类'模式, 不预占位"
      :anti-pattern "先建空 module '占位未来用' — 违反, 未来不来就是死权重 (e.g. 最初拟建的 intent-layer-bridge 后弃)"
      :trigger "≥3 表 + ≥1 trait + ≥1 稳定 writer → 开新 support module")

    (principle-8 ownership-by-usage
      :statement "跨 pillar 表的归属按 '使用方' 决定, 不按 '创建方'"
      :examples
        "TimelineStore (trait) 归 pillar 四 (event-bus projection reader), 即使 memory 的 MissionStore super-trait 包含它"
        "tasks 表 v0.5.0 D010 决策: 按 use 归 pillar 二 compute (25 caller 在那儿), memory 不管"
      :lesson "D010 教训 — v0.5.0 最初设计 agent 以为 memory 全包, 实际 pillar 二 为大头"))


  ;; ═══════════════════════════════════════════════════
  ;; 元概念 (本次引入, 未来可复用)
  ;; ═══════════════════════════════════════════════════
  (meta-concepts-introduced

    (pillar-interfaces
      :desc "pillar 对外接口的 '正交维度'. module 按业务切, surface 按消费者类别切, 两者交叉形成矩阵"
      :layers
        (surface 5 "mcp-surface / worker-trait-surface / frontend-surface / cross-pillar-surface / external-filesystem")
        (module 9 "按业务域, e.g. memory 有 5 business + 4 support")
      :binding "每 writer/reader 通过 :binds-to [:surface-name] 指向维度, 形成反向索引矩阵")

    (trait-organization-primary-vs-sub
      :primary "lisp 声明, 对外稳定契约 (e.g. ConversationStore)"
      :sub "代码层按 .rs 文件切, 可合并入 primary (e.g. ToolCallStore/EventStore/RetrospectiveStore → ConversationStore)"
      :rust-coherence "一 type 一 trait 一 impl block → sub-trait 合并后所有方法集中到一个 .rs 文件"
      :pattern "lisp 只暴露 primary, sub 是实现细节")

    (column-ownership-vs-row-ownership
      :innovation "embedding-support module 首次引入 — 0 张独占表, 但 own '列契约 + policy'"
      :use-case "跨表治理 (e.g. embedding 列分布在 5 表, 但 schema + HNSW 参数 + provider 绑定统一)"
      :dual-ownership
        (row-owned-by "承载表的业务 module")
        (column-owned-by "治理 module"))

    (ownership-by-usage-principle
      :desc "见 principle-8"
      :implications "跨 pillar 表保留在 MissionStore super-trait bound, 但 lisp 归属明确标 'owned-by pillar X, 非本 pillar'")

    (memory-hook-pipeline
      :desc "一个 pillar 可能有跨 pillar 的 '业务机制', 需在 cross-pillar-surface 声明 pipeline (多步)"
      :example "memory-hook v0.5.0: 6 步从 memory pillar (state::submit_task) → pillar 二 (memory_scheduler) 的任务触发"))


  ;; ═══════════════════════════════════════════════════
  ;; execution lisp 6-slot 模式 (沿用 board::helper agent-execution-coordination)
  ;; ═══════════════════════════════════════════════════
  (execution-lisp-schema
    :file-name-convention "intent-<pillar>-execution.lisp (和 frozen intent-<pillar>.lisp 配对)"
    :slots
      (phase-tracker "当前 phase 全局状态 + roadmap 树")
      (claims        "谁锁定了哪个 scope (防并发写冲突, 多 agent 场景)")
      (deviations    "D<NNN>: 意图 (frozen lisp) vs 实际 (code) 的差异 — agent 遇错 STOP 记录")
      (decisions     "DC<NNN>: 施工过程小决策 (非 frozen lisp 改动)")
      (completions   "comp-<NNN>: 每 stage 完成记录 (agent ID + summary + cargo 结果)")
      (issues        "I<NNN>: 阻塞/未决问题, 不阻断但需跟进")
    :retention "施工结束后归档, 不合并进 frozen lisp (frozen 保持设计真相, execution 保留施工过程)"
    :id-allocation "每 agent 写前 claim 下一个未用 ID; 串行化通过主 Claude")


  ;; ═══════════════════════════════════════════════════
  ;; Anti-Patterns (踩过的坑)
  ;; ═══════════════════════════════════════════════════
  (anti-patterns

    (anti-1 跳过-lisp-设计直接动代码
      :symptom "agent 上来就改代码, 无 frozen 蓝图"
      :consequence "改到一半迷路 + 无验收标准 + 回滚无参考点")

    (anti-2 一次性大-refactor-不分-stage
      :symptom "一个 agent 做完 9 module 合并"
      :consequence "单点 cargo build 失败 → 整片回滚; debug 成本几何上升")

    (anti-3 agent-无视-frozen-lisp-实情
      :symptom "agent 按 prompt 推进, 不看 lisp legacy-zone 实标"
      :consequence "删 '应保留' 的 (e.g. tasks 是 ✓ KEEP 但 prompt 说 drop) — agent 硬推会破系统"
      :defense "agent prompt 里显式要求 'agent 如发现任何意外, 立即 STOP 报主会话'")

    (anti-4 rust-analyzer-假警报当真
      :symptom "agent 看 rust-analyzer E0046 就慌, 乱改"
      :consequence "错误改写导致真编译失败"
      :defense "cargo build --workspace 是 ground truth. rust-analyzer 报错先 cargo 验证")

    (anti-5 预占位空-module
      :symptom "lisp 先建 module-X 占位, 想'未来用'"
      :consequence "若未来不来就成死权重. 违反 '数据先于 module'"
      :example "最初拟 intent-layer-bridge, 调研后发现 '没数据要占' → 放弃")

    (anti-6 DSL-压缩太早
      :symptom "施工时还在迭代就急着 L2/L3 压缩 lisp"
      :consequence "每次改 lisp 都要展开 macro, 施工速度反而慢"
      :defense "L1 可做 (外移 + SSOT 合并); L2/L3 等 forge 就绪")

    (anti-7 line-号写进-frozen-lisp
      :symptom "lisp 里 file:NNN 精确行号"
      :consequence "施工后 trait 重排行号必 stale"
      :defense "用 file::function_name 或 file::concept 稳定标签; 详见 path-convention")

    (anti-8 双向同步假设
      :symptom "以为 pillar 之间可双向同步 (e.g. lisp ↔ DB spec-db-sync)"
      :reality "pillar 间是单向服务方关系 (e.g. forge 为 missiond 冲压, 不接受 missiond 倒灌)"))


  ;; ═══════════════════════════════════════════════════
  ;; Artifacts 清单
  ;; ═══════════════════════════════════════════════════
  (artifacts-produced

    (frozen-design-lisp
      :name "intent-<pillar>.lisp"
      :purpose "设计真相"
      :location ".missiond/v2/")

    (history-lisp
      :name "intent-<pillar>-history.lisp"
      :purpose "外移的长 history + migration-log (减 frozen lisp 的 context)"
      :when-to-create "L1 polish 阶段 (frozen lisp 超 ~2500 行时)")

    (execution-lisp
      :name "intent-<pillar>-execution.lisp"
      :purpose "施工过程 6-slot 记录"
      :lifetime "施工中 active, 施工结束归档")

    (drop-migration
      :name "migrations/NNNNNNNN_drop_deprecated_tables.sql"
      :purpose "phase-D 执行 lisp 标的 DROP-CANDIDATE 表")

    (this-workflow-lisp
      :name "pillar-refactor.lisp (本文件)"
      :purpose "methodology, 给未来其他 pillar refactor 复用"))


  ;; ═══════════════════════════════════════════════════
  ;; Checklist for New Pillar Refactor
  ;; ═══════════════════════════════════════════════════
  (checklist-for-new-refactor

    (before-starting
      "[ ] 指挥官明确该 pillar 值得重构 (耦合严重 / 架构需升级)"
      "[ ] 已有 codebase survey (大致知道 module 边界)"
      "[ ] 目标态架构方向清晰 (即使不细)")

    (phase-A-lisp-design
      "[ ] 先写骨架 lisp (9 module + 5 surface 可用 memory pillar 为模板)"
      "[ ] 多轮 unfreeze/refreeze 迭代 (预期 5-10 版)"
      "[ ] frozen 前审计: 每 module 7 区块对称 + 所有 writer/reader :binds-to 100%"
      "[ ] 加 target-code-layout (in-scope / out-of-scope 明确)")

    (phase-B-execution-init
      "[ ] 开 intent-<pillar>-execution.lisp 6 slots"
      "[ ] 派 phase-1 scan agent (Explore), 只读不改"
      "[ ] 对 scan 报告记 D/I, 主会话决策")

    (phase-C-施工
      "[ ] 分 6 sub-stage, 每 stage 独立 agent"
      "[ ] 每 stage 后: 主 Claude cargo verify + commit + 更新 execution lisp"
      "[ ] agent prompt 明确 '遇意外 STOP 报主会话'")

    (phase-D-验证
      "[ ] binds-to cross-ref 全验证"
      "[ ] lisp↔code 双向 audit"
      "[ ] drop migration 按 lisp 标签执行 (只删 DROP-CANDIDATE)"
      "[ ] zombie trait 方法清理 (删没 caller 的)")

    (phase-E-polish
      "[ ] 施工发现的 lisp 漏洞汇总 → unfreeze 补一版"
      "[ ] 架构级新需求 → 可能跳版本 (e.g. v0.4.x → v0.5.0)"
      "[ ] L1 语义压缩 (history + 长 log 外移)")

    (after-finishing
      "[ ] frozen lisp 恢复 (下次改需明批)"
      "[ ] workflow.lisp 更新 (如果本 refactor 有新洞察)"
      "[ ] skill-store / CLAUDE.md 记忆更新"))


  ;; ═══════════════════════════════════════════════════
  ;; Reference Instance — memory pillar (详细统计)
  ;; ═══════════════════════════════════════════════════
  (reference-instance-memory
    :commit-range "fad2850 (v0.4.17) → c2f01f9 (v0.5.0 L1 polish)"
    :时间跨度 "2026-04-19 晚 → 2026-04-20 晚 (约 1-2 主会话)"

    (phase-A-output
      :版本-迭代-次数 11 "v0.4.16 → v0.4.17 → v0.4.18 → v0.4.19 → v0.4.20 → v0.4.21 → v0.4.22 → v0.4.23 (frozen) → v0.4.24 (unfreeze 补漏) → v0.5.0 → v0.5.0 L1 polish"
      :架构-定稿 "9 module × 5 surface 正交矩阵")

    (phase-C-stats
      :stage-2A "3 子动作 (删 16 gen_*.rs / KnowledgeStore rename / 删 timeline 空壳)"
      :stage-2B "2 个全新 trait (InfraStore + DirectiveLayerStore)"
      :stage-2C "5 sub-trait 合并 (SkillStore+ToolCall+Event+Retrospective+Vision)"
      :stage-2D "跨 trait 拆 24 方法 到 InfraStore"
      :stage-2E "sqlite 生态 -14018 LOC"
      :stage-2F "file-to-module-mapping-complete 文档"
      :agent-派出数 15 "comp-001 → comp-015")

    (phase-D-stats
      :drop-表 "4 张 (events/credentials/narrations 2)"
      :zombie-trait-方法 "11 个删除 (narration 9 + events 2)"
      :D-series-总 10)

    (phase-E-stats
      :unfreeze-补漏 "13 处 doc drift (v0.4.24)"
      :架构升级 "memory-hook 迁 board_tasks (v0.5.0 phased B)"
      :L1-压缩 "-380 LOC, 2999→2629")

    (总成果
      :代码净删 "30000+ LOC"
      :trait 13→9 "primary + 1 外部 (TimelineStore)"
      :最终架构 "9 module (5 business + 4 support) × 5 surface 正交"
      :D-series-总 10
      :I-series-总 7
      :commit-总 20+))


  (freeze-note
    "本 workflow 记录 memory pillar 实战后的方法论提炼."
    "未来对其他 pillar (compute/intent-layer/flow/system) 做类似重构, 先读本文件."
    "Reusable, 不和具体 pillar 耦合."))
