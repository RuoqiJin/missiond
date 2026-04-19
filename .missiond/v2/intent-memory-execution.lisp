;; ══════════════════════════════════════════════════════
;; MissionD — Memory Pillar Execution Log (施工并行 agent 共享内存层)
;; Parent:   .missiond/v2/intent-memory.lisp :: v0.4.23 frozen @ commit a282f87
;; Created:  2026-04-20
;; Purpose:  memory pillar 代码同构施工过程记录 — 6 slots (agent-execution-coordination 模式)
;;
;; 读写规则 (和 board :: helper agent-execution-coordination 一致):
;;   phase-tracker:  当前 phase 全局状态
;;   claims:         谁锁定了哪个 scope (防并发写冲突)
;;   deviations:     意图 (frozen lisp) 与实际 (code) 的差异 — 格式 D<NNN>
;;   decisions:      决策日志 — 格式 DC<NNN>
;;   completions:    phase 完成清单 — 断点续跑基础
;;   issues:         阻塞/未决问题 — 格式 I<NNN>
;;
;; ID 分配: 每个 agent 写前 claim 下一个未用 ID (串行化通过主 Claude 或 file lock)
;; frozen lisp 改动: 必须先记 deviation D<NNN>, 指挥官批准后才改 frozen lisp
;; ══════════════════════════════════════════════════════

(execution memory-pillar-isomorphism
  (parent-lisp "intent-memory.lisp :: v0.4.23 (commit a282f87)")
  (pairing-with "frozen design lisp ↔ execution log (pilot: intent-event-bus.lisp ↔ intent-event-bus-execution.lisp)")
  (created "2026-04-20")
  (scope "memory pillar 代码同构 — target-code-layout :: in-scope 5 路径")

  ;; ─────────────────────────────────────────────────────────
  ;; phase-tracker — 当前施工全局状态
  ;; ─────────────────────────────────────────────────────────
  (phase-tracker
    :current-phase "phase-1-scan"
    :started-at "2026-04-20"
    :roadmap
      (phase-1 :name "file-to-module-mapping 扫描补齐"     :status "in-progress" :deliverable "file-scan-results 槽位")
      (phase-2 :name "按 module 生成 impl-checklist"        :status "pending"     :parallelism-hint "9 module 可并行 9 agents")
      (phase-3 :name "填 DirectiveLayerStore (全新 trait)"  :status "pending"     :depends-on ["phase-1" "phase-2"])
      (phase-4 :name "binds-to cross-ref 验证"              :status "pending")
      (phase-5 :name "agent-team lisp ↔ code 同构双向校验"  :status "pending")
      (phase-6 :name "(可选) drop migration for pending-drop 表" :status "pending"))

  ;; ─────────────────────────────────────────────────────────
  ;; claims — 谁锁定了什么, 防并发写冲突
  ;; ─────────────────────────────────────────────────────────
  (claims
    ;; 示例: (C001 :claimer "explore-agent-1" :scope ["crates/missiond-core/src/db/"] :phase "phase-1" :at "2026-04-20T..." :released false)
    )

  ;; ─────────────────────────────────────────────────────────
  ;; deviations — frozen lisp 意图 vs 代码实际的差异
  ;; 记录前 frozen lisp 不能改; 记录后等指挥官批准才能改
  ;; ─────────────────────────────────────────────────────────
  (deviations
    (D001
      :lisp-said "骨架 file-to-module-mapping 列 'db/project.rs → ProjectStore' 和 'db/observability.rs → ObservabilityStore'"
      :actually-found "这 2 个 db-level 文件不存在. 实际: ProjectStore trait 定义在 db/traits.rs, impl 在 db/pg/project.rs (ObservabilityStore 同). 无 SQLite-level 中间层文件."
      :reason "lisp 骨架假设 db/<name>.rs + db/pg/<name>.rs 两层结构, 但 2 处 domain 只有 pg/ 层"
      :blocker-level "medium — 不阻断施工, 但骨架语义错"
      :approval-needed "指挥官需决定: (a) 改 frozen lisp 骨架 修正描述 [不推荐: lisp 已 frozen]; (b) 理解 'db/project.rs' 为广义域概念, 含 traits.rs 声明 + pg/ impl; (c) 施工时补 db/project.rs / db/observability.rs 中间层文件"
      :submitted-at "2026-04-20 phase-1 scan"
      :status "pending-approval")

    (D002
      :lisp-said "pillar-interfaces :: worker-trait-surface :: current-traits 列 9 traits + cross-cutting db-trait-abstraction :stores 9"
      :actually-found "crates/missiond-core/src/db/traits.rs 实际定义 14 traits (pg/ 13 impl). Extras 可能 (需细察): KnowledgeStore 独立? SkillStore 独立? ToolCallStore / RetrospectiveStore / VisionStore / EventStore?"
      :reason "v0.4.20 修正 13→9 是对齐 pillar-interfaces 列表, 但 pillar-interfaces 列表本身可能不全; 或代码有 9 外的次级 trait"
      :blocker-level "high — 影响 施工 phase-2 的 impl-checklist 生成"
      :approval-needed "指挥官需决定: (a) 重扫代码确认准确 trait count; (b) 补齐 pillar-interfaces current-traits 到 14; (c) 还是确认 lisp 的 9 大 trait 是 'primary/public', 其余 5 是 'internal/helper' 不列"
      :submitted-at "2026-04-20 phase-1 scan"
      :status "pending-approval"))

  ;; ─────────────────────────────────────────────────────────
  ;; decisions — 决策日志 (非 frozen lisp 改动, 施工过程的小决策)
  ;; ─────────────────────────────────────────────────────────
  (decisions
    (DC001
      :context "施工开工"
      :rationale "v0.4.23 frozen @ a282f87 后开此 execution lisp 作为并行 agent 共享内存层"
      :decided-by "指挥官 + Claude Opus 4.7"
      :at "2026-04-20"))

  ;; ─────────────────────────────────────────────────────────
  ;; completions — phase 完成清单
  ;; ─────────────────────────────────────────────────────────
  (completions
    (comp-001
      :phase "phase-1-scan"
      :agent "explore-agent (main session dispatched)"
      :summary "扫 in-scope 路径完成; 14 骨架文件 12 match 2 mismatch; 发现 22 wild files; trait count 9 vs 14 discrepancy"
      :blockers-raised ["D001" "D002"]
      :issues-raised ["I001" "I002" "I003"]
      :deliverable-location "file-scan-results 槽位 (此 execution lisp)"
      :at "2026-04-20"
      :next-phase-blocked-on "D001 + D002 指挥官决策 → 然后 phase-2 impl-checklist"))

  ;; ─────────────────────────────────────────────────────────
  ;; issues — 阻塞 / 未决问题
  ;; ─────────────────────────────────────────────────────────
  (issues
    (I001
      :severity "medium"
      :desc "22 个 'wild files' 未列在 lisp 骨架 file-to-module-mapping, 包括: audit.rs / beacon.rs / shared.rs / error.rs / conversation_query.rs / message_feed.rs / router_chat.rs / timeline.rs(deprecated) / narration.rs / translation.rs / watermark.rs / dynamic_slot.rs / executor.rs / mod.rs / migration.rs + 10 个 gen_*.rs (forge 冲压产物)"
      :resolution-path "phase-1 完成 deliverable 包含补分类; 每文件归到对应 module 或标 '辅助/子功能/codegen'"
      :not-blocker "属于补全动作, 不阻断施工"
      :at "2026-04-20 phase-1 scan")

    (I002
      :severity "low"
      :desc "timeline.rs 已 deprecated (v1.3.0 event_log 取代), 但文件仍在代码里仅做 re-export"
      :lisp-standing "lisp 已在 pillar-interfaces / retention-policy 多处标明 timeline SSOT 迁 event_log; 但 timeline.rs 本身的 drop 未入 phase-6 清单"
      :resolution "phase-6 drop migration 时一并清 (或单独 track)"
      :at "2026-04-20 phase-1 scan")

    (I003
      :severity "info"
      :desc "migrations/ 实际 22 个 .sql 文件, lisp 称 60 表. agent 口径 '61 张实际' 可能把 drop_system_timeline 也算了. 需 phase-4 精确核对"
      :at "2026-04-20 phase-1 scan"))

  ;; ─────────────────────────────────────────────────────────
  ;; phase-1 产出: file-to-module-mapping 扫描结果
  ;; 扫完后补填; 完成后主 Claude 审核 → 记 completion → 进 phase-2
  ;; ─────────────────────────────────────────────────────────
  (file-scan-results
    :status "phase-1 scan 完成 (agent explore-1 于 2026-04-20)"
    :target-sync "intent-memory.lisp :: target-code-layout :: file-to-module-mapping"

    (骨架-14-match-summary
      (match  12 "ast.rs / board.rs / conversation.rs / gemini_log.rs / incident.rs / knowledge.rs / question.rs / skill.rs / slot.rs / task.rs / backfill.rs / traits.rs")
      (mismatch 2 "db/project.rs + db/observability.rs 不存在 (见 D001)")
      :mismatch-resolution "待 D001 决策")

    (extras-unaccounted-22
      :note "lisp 骨架未列, 本次扫描发现的 in-scope 文件; 需补入 mapping"

      (helper-support
        (audit.rs       :serves "conversation-logs" :trait "ToolCallStore (混合)" :note "tool call 审计")
        (beacon.rs      :serves "kb-manager"        :note "beacon CRUD (kb-manager 子功能)")
        (shared.rs      :serves "all modules"       :note "Row struct + enum shared types")
        (error.rs       :serves "all modules"       :note "DbError type"))

      (query-view-helpers
        (conversation_query.rs :serves "conversation-logs" :note "conversation FTS query builder")
        (message_feed.rs       :serves "conversation-logs" :note "message stream view")
        (router_chat.rs        :serves "system-support"    :note "router chat state (配套 router_chat_archive)"))

      (legacy-infra
        (timeline.rs    :status "DEPRECATED (v1.3.0 后仅 re-export shared types)"
                        :owned-by "system-support legacy (候选 drop)")
        (narration.rs   :status "LEGACY (narration 表 pending-drop-v0.4.12)"
                        :owned-by "conversation-logs pending-drop")
        (translation.rs :serves "conversation-logs" :note "message_translations 表支持")
        (watermark.rs   :serves "system-support"    :note "消息 watermark tracking")
        (dynamic_slot.rs :serves "slot-support"     :note "dynamic_slot state 管理细节")
        (executor.rs    :serves "slot-support (间接)" :note "executor lifecycle"))

      (infra-module
        (mod.rs         :serves "all modules"  :note "db/ 模块定义")
        (migration.rs   :serves "all modules"  :note "migrations runner infra"))

      (forge-generated-10
        :note "forge 冲压产物, 当前 gitignored. 施工时按对应 module 分组"
        (gen_audit.rs       :module "conversation-logs (audit)")
        (gen_board.rs       :module "board")
        (gen_compute.rs     :module "slot-support")
        (gen_conversation.rs :module "conversation-logs")
        (gen_knowledge.rs   :module "kb-manager")
        (gen_misc.rs        :module "system-support")
        (gen_pipeline.rs    :module "?? (需细察)")
        (gen_skill.rs       :module "project-management")
        :missing-2 "agent 列 8 but 上面 claim 10; 需 phase-2 精确核对"))

    (pg-impl-alignment
      :status "pg/*.rs 13 文件对应 14 trait impl"
      :files "pg/{board,conversation,event,knowledge,message,observability,project,retrospective,skill,slot,timeline,tool_call,vision}.rs"
      :note "observability 只在 pg/ 有 (见 D001); vision pg impl 存在但 trait 未在 lisp 9 list")

    (trait-count-discrepancy
      :lisp-says 9
      :agent-reports 14
      :extras-guess "KnowledgeStore(vs KbStore) / SkillStore / ToolCallStore / RetrospectiveStore / VisionStore / EventStore — 需 phase-2 精确核对"
      :see D002))

  ;; ─────────────────────────────────────────────────────────
  ;; phase-2+ 产出预留 (施工过程中填)
  ;; ─────────────────────────────────────────────────────────
  (module-impl-checklists
    :status "pending phase-2")

  (directive-layer-implementation
    :status "pending phase-3"
    :required-trait "DirectiveLayerStore (15 方法)"
    :required-files ["db/directive.rs" "db/pg/directive.rs" "types/directive.rs"])

  (binds-to-verification
    :status "pending phase-4"
    :target "96 个 writer/reader 的 :cross-ref 指向真实代码函数")

  (isomorphism-audit
    :status "pending phase-5"
    :bidirectional "lisp → code + code → lisp")

  (drop-migration-plan
    :status "pending phase-6"
    :candidates ["message_narrations" "narration_cursors" "tasks" "inbox" "events" "credentials"]
    :note "lisp 已标 pending-drop; migration 待写"))
