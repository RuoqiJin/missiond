;; MissionD — Workflow: 大型基础设施重构 (Bus Refactor 范式)
;; Captured: 2026-04-19
;; Concrete instance: event-bus refactor (branch refactor/event-bus-v2, commits 49e10be..e139ecf)
;;
;; 用途:未来遇到"推倒重来"级的基础设施重构(非局部 bug fix / feature add),
;;       参考本 workflow 复用方法论,避免重复踩坑。
;;
;; 读这份 lisp 的时机:
;;   - 起草大型重构计划前
;;   - agent 执行中遇到规模管理 / 偏离记录 / 并行编排的问题时
;;   - 想上架构冻结锁时

(workflow bus-refactor
  :schema "missiond.workflow.methodology.v1"
  :workflow_id bus-refactor
  :status historical-methodology
  :source_plans [event-bus-v2-refactor]
  :steps [s1 s2 s3 s4 s5]
  :risk-gates [manual-review-only no-runtime-autonomous-execution]
  :completion (:checks ["methodology reference only; not a live execution contract"]
               :artifact "historical bus refactor workflow")
  (granularity meta-methodology)
  (reusable-for "事件总线 / DB 层 / IPC 协议 / 状态机 / 跨 crate 抽象层 等'神经中枢级'重构")
  (not-for "局部 bug fix / 单文件重构 / feature add")

  ;; ═══════════════════════════════════════════════════
  ;; 元模式:五大阶段
  ;; ═══════════════════════════════════════════════════
  (meta-phases 5
    (phase-A exploration
      :goal       "充分 survey 现状,让架构设计基于事实不基于想象"
      :output     "inventory 文档 + 当前架构的 lisp 描述"
      :typical-cost "10-20% 总时间"
      :tools      "Explore agent / Grep / Read"
      :skip-cost  "设计和现实脱节,实施时发现 lisp 描述错,被迫返工")

    (phase-B design-freeze
      :goal       "拍定所有架构决策,写进 lisp 冻结"
      :output     "frozen lisp(含 :decided-options / :invariants / 7-step pipeline 等)"
      :typical-cost "15-25% 总时间"
      :critical   "决策前可开放讨论 / 采纳多方意见;决策后冻结"
      :tools      "用户拍板 + 外部审议(gemini.md / 历史记录.md)+ Claude 综合")

    (phase-C scaled-execution
      :goal       "按 lisp 规格分 phase 实施,每 phase 独立 agent 完成"
      :output     "代码 + 测试 + 累积 execution lisp 记录"
      :typical-cost "50-60% 总时间"
      :pattern    "每 phase = 一个后台 agent + 独立 commit + 测试零回归门槛")

    (phase-D physical-alignment
      :goal       "代码文件树 1:1 映射 lisp 组件层级(物理同构)"
      :output     "pipeline/stepN_*/ 类目录 + 每 component 精确 :target"
      :typical-cost "10-15% 总时间"
      :value      "未来 agent 读 lisp 即可直接定位代码,减少 survey 步数")

    (phase-E lock-and-polish
      :goal       "冻结锁 + god-file 拆分 + 诚实记录偏离"
      :output     "file-governance 块 + 拆分提交 + deferred 清单"
      :typical-cost "5-10% 总时间"
      :catch      "偏离不要隐藏,写进 execution lisp deviations"))

  ;; ═══════════════════════════════════════════════════
  ;; 七项核心原则
  ;; ═══════════════════════════════════════════════════
  (principles
    (principle-1 lisp-first
      :rule   "所有架构决策先落 lisp,代码再跟随"
      :why    "文字契约比口头约定能跨 session 持续,agent 接力靠它"
      :counter-example "先写代码再补文档 → 文档永远滞后 → agent 读不懂真意")

    (principle-2 agent-team-by-default
      :rule   "耗时 > 5 分钟的任务派后台 agent,保护主会话上下文"
      :why    "主会话做编排 / 审阅 / 决策;agent 做执行 / grep / 编译"
      :anti-pattern "agent 产出全文 dump 回主会话 → 上下文爆炸"
      :correct-pattern "agent 只回一句摘要 + 产物文件路径;主会话按需 Read")

    (principle-3 execution-lisp-as-shared-memory
      :rule   "建一份配套的 execution lisp 记 phases / deviations / decisions / claims"
      :why    "多 agent 并行 / 跨 session 接力,需要共享状态"
      :fields-needed
        ("(phases ...)    各 phase 状态 + owner + summary"
         "(claims ...)    并行 agent 占用文件范围,防冲突"
         "(deviations ...) 实际偏离 frozen lisp 的记录"
         "(decisions ...) frozen lisp 未覆盖的次级决定"
         "(completions ...) 每 phase 交付的文件清单 + 测试状态"
         "(issues ...)    未决阻塞 + severity"))

    (principle-4 zero-regression-gate
      :rule   "每 phase commit 前必须:cargo build clean + 测试无回归"
      :why    "重构中间态不能破坏已有功能,否则下一 phase 基础不稳"
      :enforcement "每 phase agent 必须自验证,把 tests pass 数据写进 completion")

    (principle-5 physical-isomorphism
      :rule   "lisp 组件层级 ↔ 代码目录层级 ↔ 文件内类型 — 三层同构"
      :why    "读 lisp 就是读代码目录,未来 Claude Code 0 survey 步数"
      :example "lisp §4.2 step-3 commit / dedup-semantics → pipeline/step3_commit/dedup.rs")

    (principle-6 ask-before-edit-frozen
      :rule   "frozen lisp 上锁后,agent/LLM 改动必须先问用户"
      :why    "frozen = 架构契约 = 下游所有 agent 的真理源,擅改 = 协议违规"
      :four-layers
        ("layer-1-banner:        顶部巨型警告"
         "layer-2-governance:    机器可读 (file-governance :lock frozen)"
         "layer-3-fs-permission: 可选 chmod 444 OS 级只读"
         "layer-4-memory-entry:  ~/.claude 项目记忆长期提醒"))

    (principle-7 honest-deviation-logging
      :rule   "发现 frozen lisp 与现实矛盾 → 记 deviation,不静默兜底"
      :why    "未来 agent 读 lisp + execution 能看到全部真相"
      :format "deviation :id Dxxx :phase N :lisp-said '...' :actually-did '...' :reason '...'"))

  ;; ═══════════════════════════════════════════════════
  ;; 产物清单(每次 bus-refactor 类重构的完整交付)
  ;; ═══════════════════════════════════════════════════
  (artifacts
    (architecture-lisp
      :filename  "intent-<subsystem>.lisp"
      :status    "frozen + locked"
      :contains  ":decided-options / :design-philosophy / pipeline steps / :target 全覆盖")

    (execution-lisp
      :filename  "intent-<subsystem>-execution.lisp"
      :status    "mutable + append-only"
      :contains  "phases / claims / deviations / decisions / completions / issues")

    (inventory-md
      :filename  "_phaseX-inventory.md"
      :purpose   "Phase 0 survey 产物,列出所有 touch 点的 file:line")

    (refactor-summary-md
      :filename  "_refactor-summary.md"
      :purpose   "Phase 最终验证交付,含 ASCII 架构图 before/after + 数字统计")

    (feature-branch
      :name-pattern "refactor/<subsystem>-v<N>"
      :merge-strategy "--no-ff 保留 phase 提交历史"
      :commits "每 phase 一 commit + lisp 演进 commits,一般 10-20 commits"))

  ;; ═══════════════════════════════════════════════════
  ;; 11 phases 模板(具体实例化:event-bus refactor 已用此范式)
  ;; ═══════════════════════════════════════════════════
  (phase-template 11

    (phase-0 survey
      :goal     "Survey 现存代码,产出 inventory"
      :agent    "1 个 Explore / general-purpose agent"
      :duration "10-15 分钟"
      :deliverable "_phase0-inventory.md 覆盖所有 publish/subscribe/MPSC/表的 file:line"
      :skip-risk  "后续 phase 凭想象实施,90% 返工")

    (phase-1 schema-layer
      :goal     "定义新类型体系 — trait + 核心 enum / struct"
      :agent    "1 个 general-purpose"
      :coexist  "与旧类型共存,不改旧"
      :tests    "每 type 单元测试:domain/kind/serde round-trip")

    (phase-2 storage-layer
      :goal     "持久化层 — 数据库 schema + migrations + 主 writer"
      :agent    "1 个"
      :includes "writer + backend trait + production impl + backpressure + dedup + retry")

    (phase-3 routing-layer
      :goal     "路由 / dispatch / control-gate"
      :agent    "1 个"
      :state-budget "O(1) — 不给每个消费者维护状态")

    (phase-4 consumer-api-layer
      :goal     "消费端 API + 订阅 / cursor / 组合子"
      :agent    "1 个"
      :critical-doc "at-least-once / cursor flush / pause behavior")

    (phase-5 cross-cutting
      :goal     "InMemoryBus + chaos tests + metrics"
      :agent    "1 个"
      :chaos-matrix "至少 9 个故障模式:timeout/panic ×3 tiers/slow/disconnect/loop/dedup/orphan")

    (phase-6 producer-migration
      :goal     "旧发送点 dual-emit 改走新 API(保留旧路径不删)"
      :agent    "1 个"
      :pattern  "dual-emit 过渡期:新旧并行,下游不感知")

    (phase-7 subscriber-migration
      :goal     "旧订阅者改走新 API(保留旧订阅)"
      :agent    "1 个"
      :pattern  "dual-consume 过渡期:v2 订阅建立在 v1 旁边")

    (phase-8 legacy-cleanup
      :goal     "删除旧代码 + 保留外部契约字节级兼容"
      :agent    "1 个"
      :external-contract "前端 WS / 外部 API:byte-equivalence 测试覆盖")

    (phase-9 e2e-verification
      :goal     "golden path E2E test + daemon smoke start + refactor summary"
      :agent    "1 个"
      :deliverable "_refactor-summary.md")

    (phase-10 physical-reorg
      :goal     "代码文件树按 lisp 组件层级重排 — 物理同构"
      :agent    "1 个"
      :caution  "纯结构重排不改逻辑;公共 API 100% 兼容 via pub use shim")

    (phase-11 god-file-split
      :goal     "god file 拆分 — 单文件 < 300 行(不计 inline tests)"
      :agent    "1-2 并行 agent(按目录分区避免冲突)"
      :threshold "> 500 行 + 多 trait/impl 并存 = 拆分信号"))

  ;; ═══════════════════════════════════════════════════
  ;; 并行 agent 编排模式(Phase 11 中首次验证)
  ;; ═══════════════════════════════════════════════════
  (parallel-agent-pattern
    (when-parallel
      :condition-1 "两任务目录完全独立(不共享写入文件)"
      :condition-2 "两任务依赖链无交叉"
      :condition-3 "执行 lisp 的 claim 机制能标识谁在哪"
      :example "Phase 11 α=pipeline/** vs β=subscription/** + in_memory/**")

    (claim-protocol
      :entry "agent 开始时写 (claim :phase N :scope \"...\" :agent \"...\" :claimed-at ...)"
      :exit  "完成时写 :released-at;未完成放 :released-at nil 但 :status 标明"
      :conflict-detection "启动前读 claims,若 scope 重叠 → 拒绝启动 / 告警")

    (shared-file-serialization
      :rule  "execution lisp 这类共享文件,并行 agent 应在不同 section 追加"
      :pattern "α 加 deviations D008-D009,β 加 D010-D012;不改彼此 ID 段"))

  ;; ═══════════════════════════════════════════════════
  ;; 决策权限分层(关键!)
  ;; ═══════════════════════════════════════════════════
  (decision-authority
    (user-only
      (architecture-pillar-change "新板块 / 拆板块 / 重排板块结构")
      (frozen-option-change "decided-options 任一值变更")
      (invariant-weakening "放松任何 invariant / contract / guarantee")
      (scope-expansion "加入原 :out of scope 项")
      (lock-release "frozen lisp 解锁 / 降版本")
      (destructive-ops "force push / reset --hard / 数据迁移"))

    (agent-autonomous
      (typo-fix "标点 / 错别字")
      (path-drift-correct "代码移动后更新 :target")
      (new-target-addition "新增子模块补 :target")
      (internal-refactor "同层代码拆 / 合,不改 API")
      (test-coverage "加测试不改行为")
      (doc-enhancement "加 :scope / :rationale 字段 强化 lisp")
      (execution-lisp-append "phases / deviations / decisions 追加"))

    (agent-must-ask
      (ambiguity-in-frozen-lisp "frozen lisp 自相矛盾 / 漏定义")
      (phase-scope-expansion "phase 实施时发现需改其他 phase 范围")
      (external-contract-change "影响前端 / MCP / 任何外部消费者的 wire format")
      (new-dependency "引入新 crate / 新 feature flag")
      (migration-of-user-data "迁移已有生产数据")))

  ;; ═══════════════════════════════════════════════════
  ;; 反模式清单(踩过的坑 / 看到的坑)
  ;; ═══════════════════════════════════════════════════
  (anti-patterns
    (patch-based-design
      :smell   "sweeper 补漏 / 轮询兜底 / fallback 掩盖"
      :why-bad "承认 broadcast 丢事件 → 补写个扫描器 = 设计自证失败"
      :fix     "重新审视根本:是不是抽象选错了?(如 broadcast 应换成 persistent log)")

    (cosmetic-split
      :smell   "Phase 10 拆 step3_commit 却留 log_writer 1008 行"
      :why-bad "doc-anchor 文件是纸面的,真实代码还是 god file"
      :fix     "拆要拆代码,不只拆 :target 字段;加 :scope 字段明确各文件职责")

    (silent-deviation
      :smell   "实施时觉得 lisp 错了,自己偷改"
      :why-bad "下游 agent 读 lisp 仍看老规格,基于错的假设干活"
      :fix     "STOP → 记 execution lisp issue → 询问用户 → 授权后再动")

    (dump-into-main-context
      :smell   "agent 做 30 分钟 survey,把 3000 字 inventory dump 回主会话"
      :why-bad "主会话上下文爆炸,无法持续"
      :fix     "agent 写 inventory 到文件,只回一句摘要 + 路径")

    (big-bang-no-phases
      :smell   "一个 phase 做 10 phase 的量"
      :why-bad "中间态不可审 / 不可回滚 / 测试范围太大"
      :fix     "粒度控制:每 phase 3-7 个子任务,跑 1-2 session 完成")

    (god-file-accretion
      :smell   "单文件 > 500 行,多 trait/struct/impl 无 section 分隔"
      :why-bad "维护时心智成本高,agent 读困难"
      :fix     "拆:按 abstraction / impl / orchestration 分层")

    (frozen-without-lock
      :smell   "lisp 写了 frozen 但没锁机制"
      :why-bad "口头约定 → agent 照改不误"
      :fix     "加 (file-governance :lock frozen) + banner + 记忆条目"))

  ;; ═══════════════════════════════════════════════════
  ;; 数字基线(event-bus refactor 实测)
  ;; ═══════════════════════════════════════════════════
  (baseline-numbers-event-bus-refactor
    :total-commits 16
    :total-phases  12        ;; 0-11
    :lisp-revisions "v0 draft → v1.0.0 frozen → v1.1.0 god-file split"
    :code-changes "155 files,+29909 / -3398"
    :test-count-before 0     ;; 旧 bus 几乎无单测
    :test-count-after 363    ;; 255 core + 12 chaos + 96 daemon
    :variants-before 52      ;; DaemonEvent god enum
    :variants-after 64       ;; 12 domain enums
    :agent-launches-total 25 ;; 粗略估计跨全程
    :execution-lisp-entries
      (phases 11
       deviations 12
       decisions 54
       completions 11))

  ;; ═══════════════════════════════════════════════════
  ;; 复用本 workflow 的 checklist
  ;; ═══════════════════════════════════════════════════
  (checklist-for-new-refactor
    ("□ 确认触发条件:'推倒重来'级 vs 局部 fix — 本 workflow 只适后者"
     "□ 起 feature 分支 refactor/<subsystem>-v<N>"
     "□ 建 intent-<subsystem>.lisp 草稿(先不锁)"
     "□ 跑 Phase 0 survey agent,产 _phase<N>-inventory.md"
     "□ 用户 + 外部审议敲定架构(7 关键决策 —— 对 bus 类:seq/topic/delivery/cursor/pause/payload/recovery)"
     "□ lisp 加 (decided-options) 冻结 + bump version"
     "□ 起 intent-<subsystem>-execution.lisp 作共享内存"
     "□ Phase 1-N:每 phase 一 agent,背景运行,独立 commit,零回归"
     "□ Phase N+1:E2E + smoke + summary"
     "□ Phase N+2:物理 reorg,代码 1:1 映射 lisp"
     "□ Phase N+3:god file 体检 + 拆分"
     "□ 加 (file-governance :lock frozen) + banner + memory 条目"
     "□ merge --no-ff 保留 phase 历史"))

  ;; ═══════════════════════════════════════════════════
  ;; 引用:本次 event-bus refactor 的产物
  ;; ═══════════════════════════════════════════════════
  (reference-instance
    :branch           "refactor/event-bus-v2 (merged to main 2026-04-19)"
    :frozen-lisp      ".missiond/v2/intent-event-bus.lisp"
    :execution-lisp   ".missiond/v2/intent-event-bus-execution.lisp"
    :phase-0-inventory ".missiond/v2/_phase0-inventory.md"
    :refactor-summary  ".missiond/v2/_refactor-summary.md"
    :phase-9-layout    ".missiond/v2/_phase9-layout-check.md"
    :final-merge       "commit e139ecf"))
