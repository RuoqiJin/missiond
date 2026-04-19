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
    :current-phase "phase-2-stage-2A.1"
    :started-at "2026-04-20"
    :strategy "指挥官选 A: 稳扎稳打分批. 老代码耦合严重, lisp 新架构优雅. 稳不出错就是快. 派 agent-team 保护主会话 context"
    :roadmap
      (phase-1 :name "file-to-module-mapping 扫描补齐"     :status "completed"   :delivered "comp-001 + comp-002 + comp-003; D001/D002/I001-I007 全部闭环")
      (phase-2 :name "代码向 lisp 对齐 (6 stage 分批)"      :status "in-progress"
        :stages-order "2A 热身 → 2B 新建 trait 壳 → 2C 合并子 trait → 2D 跨 trait 拆分 → 2E sqlite 生态清理 → 2F wild files 补分类"
        (stage-2A "轻量独立动作 (热身)"
          (2A.1 "删除两套 gen_*.rs (16 文件 8302 LOC ~285KB, 未被 mod.rs 导入)" :status "completed" :at "2026-04-20" :cargo-build "通过")
          (2A.2 "kb-manager: KnowledgeStore → KbStore rename"                    :status "completed" :at "2026-04-20" :cargo-build "通过")
          (2A.3 "TimelineStore 归属重审 + 删 db/timeline.rs 空壳"
            :status "completed"
            :correction "**原前提错** — Stage 2A.3 原说 'TimelineStore deprecated 应删'. 调研发现 TimelineStore 是 pillar 四 event-bus 的 projection 读接口 (见 intent-event-bus-execution.lisp L170), 不是 memory pillar 的 trait"
            :actually-done "(a) 保留 TimelineStore trait 原样 (归 pillar 四 所有); (b) 删 db/timeline.rs (551B 冗余空壳 — re-export 已由 db/mod.rs:61 shared:: 替代); (c) mod.rs 删掉 sqlite-gated 'pub(crate) mod timeline;' 一行"
            :deviation-from-plan "D003"
            :at "2026-04-20" :cargo-build "通过")
          (2A.4 "I001 wild files 文档分类 → stage-2F (主会话批量补执行 lisp)"    :status "moved-to-2F"))
        (stage-2B "新建 trait 壳 — InfraStore + DirectiveLayerStore 空壳先建"  :status "in-progress"
          (2B.1 "新建 InfraStore trait 空壳 (方法 Stage 2D 填)"  :status "completed"
            :location "traits.rs:719-730 + MissionStore bound :762 + pg/infra.rs 新建 + sqlite/mod.rs:68-71"
            :rust-analyzer-note "rust-analyzer 报 E0277 trait has no implementations, 但 cargo build --workspace 通过 — 是 IDE feature-detection 缓存问题, 不阻断施工"
            :at "2026-04-20")
          (2B.2 "新建 DirectiveLayerStore (trait + 3 文件 + 17 方法 PG impl)"
            :status "completed"
            :files-created ["types/directive.rs(189L)" "db/directive.rs(23L)" "pg/directive.rs(372L)"]
            :cargo-build "workspace 通过"
            :side-effect "Cargo.toml 加 'serde' 到 uuid features (DTO 需 Serialize)"
            :at "2026-04-20"))
        (stage-2C "合并子 trait (SkillStore / ToolCall / Event / Retrospective / Vision)" :status "in-progress"
          (2C.1 "SkillStore → ProjectStore (25+8=33 方法) + 补建 db/project.rs" :status "completed" :at "2026-04-20" :cargo "通过")
          (2C.2 "ToolCallStore → ConversationStore (19 方法)" :status "completed" :at "2026-04-20")
          (2C.3 "EventStore → ConversationStore (7 方法)" :status "completed" :at "2026-04-20")
          (2C.4 "RetrospectiveStore → ConversationStore (15 方法)" :status "completed" :at "2026-04-20")
          (2C.5 "VisionStore → ObservabilityStore (10 方法实测) + 补建 db/observability.rs"
            :status "completed" :at "2026-04-20"
            :note "agent 实测 10 方法非预期 8 (7 image + 3 translation). ObservabilityStore 最终 80 方法"))
        (stage-2D "跨 trait 拆分 (watermarks/backfill/daemon_state → InfraStore)" :status "completed" :at "2026-04-20"
          :migration-stats "InfraStore 0→24 方法, ObservabilityStore 80→58 (-22), SlotStore 40→38 (-2)"
          :files "pg/infra.rs 14→386L, pg/observability.rs -335L, pg/slot.rs -26L")
        (stage-2E "清理 sqlite 生态 (一次性大扫除)" :status "completed" :at "2026-04-20"
          :deleted "sqlite/ 整目录 10 文件 + db/ 下 21 个 sqlite-gated 文件 (含 migration.rs 1675L) = 31 文件 ~14018 LOC ~360KB"
          :migrate-tool-kept "pg/migrate_from_sqlite.rs 保留, feature-gated 'all(sqlite, postgres)' 当前永不编译, 加 DEPRECATED doc"
          :src-rewrites "inbox.rs/slot_manager.rs/mission_control.rs/learned_permissions.rs/main.rs 去 sqlite 分支; types/board.rs 去 rusqlite impl; db/error.rs 去 DbError::Sqlite 变体"
          :cargo-toml "core 删 sqlite feature + rusqlite; daemon 删 sqlite feature (codex 仍 rusqlite)"
          :db-mod-rs "422→58 行 (压缩 86%)"
          :cargo-build "--workspace 0 error; --tests --no-run 0 error")
        (stage-2F "I001 wild files 补分类"                                       :status "pending"))
      (phase-3 :name "(deprecated — 合并进 phase-2 stage-2B + 独立)"         :status "merged-into-phase-2")
      (phase-4 :name "binds-to cross-ref 验证"                                :status "pending")
      (phase-5 :name "lisp ↔ code 双向同构校验"                               :status "pending")
      (phase-6 :name "(可选) drop migration (narrations 2 + legacy 4)"       :status "pending"))

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
      :ground-truth-verified "主 Claude 用 ls 验证: db/project.rs 不存在, db/observability.rs 不存在. 仅 pg/project.rs + pg/observability.rs 存在."
      :actually-found "ProjectStore trait 定义在 db/traits.rs:724, impl 在 db/pg/project.rs (550+ 行); ObservabilityStore trait 定义在 traits.rs:614, impl 在 db/pg/observability.rs (1200+ 行). **无 db-level 中间层文件**"
      :reason "lisp 骨架假设两层 db/<name>.rs + db/pg/<name>.rs, 但这 2 处 domain 实际只有单层 (trait 在 traits.rs, impl 在 pg/)"
      :decided "指挥官指示: 代码向 lisp 对齐. 应建 db/project.rs 和 db/observability.rs 中间层文件, 承载本 domain 的 Row struct + enum + 任何非 PG-specific 的逻辑 (follow db/board.rs / db/conversation.rs 等现有 pattern)"
      :决策-方向 "(c) 施工时补建 — 先看其他 db/<name>.rs 内容模式, 按样补 db/project.rs 和 db/observability.rs"
      :施工-action "phase-2 按 module 重构时顺便建这 2 文件"
      :status "approved-decided"
      :at "2026-04-20 phase-1.5")

    (D002
      :lisp-said "pillar-interfaces :: worker-trait-surface :: current-traits 列 9 traits + cross-cutting db-trait-abstraction :stores 9"
      :ground-truth-verified "主 Claude + phase-1.5 agent 核实: traits.rs 实际 13 active trait (非 14)"
      :code-13-traits
        (ConversationStore    :行 27   :方法 56 :lisp-有 "ConversationStore")
        (MessageStore         :行 159  :方法 15 :lisp-有 "MessageStore")
        (ToolCallStore        :行 215  :方法  8 :lisp-无 "lisp 未列为 primary, 是 ConversationStore 的 sub-trait")
        (EventStore           :行 245  :方法  6 :lisp-无 "同上, conversation-logs sub-trait")
        (RetrospectiveStore   :行 261  :方法 13 :lisp-无 "同上, conversation-logs sub-trait")
        (VisionStore          :行 288  :方法  8 :lisp-无 "system-support sub-trait (image_descriptions), 应合并 ObservabilityStore")
        (KnowledgeStore       :行 308  :方法 56 :lisp-有-rename "lisp 叫 KbStore, 代码叫 KnowledgeStore — 命名不符!")
        (BoardStore           :行 396  :方法 41 :lisp-有 "BoardStore")
        (TimelineStore        :行 462  :方法  6 :lisp-无 "v0.4.20 误判为 deprecated; 实际是 pillar 四 event-bus projection 读接口 (event-bus-execution.lisp L170 声明重构为 projection delegate); 归 pillar 四, 不是 memory trait; 保留原样")
        (SlotStore            :行 478  :方法 41 :lisp-有 "SlotStore")
        (SkillStore           :行 540  :方法 25 :lisp-无 "lisp 把 skill 归 ProjectStore; 代码是独立 trait")
        (ObservabilityStore   :行 614  :方法 75 :lisp-有 "ObservabilityStore")
        (ProjectStore         :行 724  :方法  7 :lisp-有 "ProjectStore (只 7 方法; skill_* 4 表方法在 SkillStore 里)")
      :lisp-3-traits-code-没
        (KbStore              :status "rename needed — 代码叫 KnowledgeStore")
        (DirectiveLayerStore  :status "TBD — v0.4.17 声明, 未实现, phase-3 建")
        (InfraStore           :status "⚠ 代码没有! watermarks/backfill/daemon_state 方法散在 ObservabilityStore + SlotStore 里")
      :reason "lisp '9 primary traits' 是简化公开视图; 代码按 .rs 文件切粒度更细, 产生 13 active trait (含 5 个 lisp 视为 sub-trait 的 + 命名差异 1 + deprecated 1)"
      :decided "指挥官指示: 代码向 lisp 对齐. 代码重构向 9 primary traits 合并"
      :决策-方向
        (rename-KnowledgeStore→KbStore   :priority P1 :effort small)
        (delete-TimelineStore              :priority P1 :effort small :已 deprecated)
        (merge-SkillStore-into-ProjectStore :priority P2 :effort medium :lisp-说 "skill_* 4 表归 ProjectStore")
        (merge-ToolCallStore-into-ConversationStore :priority P2 :effort medium)
        (merge-EventStore-into-ConversationStore    :priority P2 :effort medium)
        (merge-RetrospectiveStore-into-ConversationStore :priority P2 :effort small)
        (merge-VisionStore-into-ObservabilityStore  :priority P2 :effort small)
        (建-InfraStore                    :priority P1 :effort medium
                                          :from "从 ObservabilityStore 拆 watermarks+backfill; 从 SlotStore 拆 daemon_state"
                                          :lisp-说 "InfraStore 归 system-support: infrastructure_state + backfill_* + daemon_state")
        (建-DirectiveLayerStore           :priority P1 :effort large "phase-3 专项, 新建 15 方法")
      :status "approved-decided"
      :at "2026-04-20 phase-1.5")

    (D003
      :lisp-said "Stage 2A.3 原计划 delete TimelineStore (基于 '它已 deprecated' 的假设)"
      :actually-found "TimelineStore 不是 deprecated; 它是 pillar 四 event-bus 的 projection 读接口 (event-bus-execution.lisp L170 明确声明 'db/pg/timeline.rs 重写: 每个 TimelineStore 方法 2-3 行 delegate 到 projection::*'); 被 ws/server.rs catch-up 和 handlers/comm/timeline.rs 的 5 MCP 工具 active 调用"
      :reason "phase-1.5 agent 对 traits.rs 13 trait 的分类把 TimelineStore 标 deprecated 是错的; TimelineStore 归 pillar 四 不归 memory"
      :decided "Stage 2A.3 动作修正: 保留 TimelineStore trait + 删 db/timeline.rs 冗余空壳 (551B re-export, 已被 shared:: 替代)"
      :scope-clarification "pg/timeline.rs / sqlite/timeline.rs / TimelineStore trait 全部归 pillar 四 event-bus 管, memory pillar 同构不动"
      :impact "memory pillar D002 的 13 code traits 重新计数: 9 primary 属 memory + DirectiveLayerStore TBD + 3 sub-traits (ToolCall/Event/Retrospective 合并 ConversationStore) + ...; 注意 TimelineStore 属 pillar 四 不计入 memory"
      :status "approved-decided"
      :at "2026-04-20 stage-2A.3"))

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
      :at "2026-04-20")

    (comp-002
      :phase "phase-1.5-deep-trait-investigation"
      :agent "explore-agent (dispatched after I001+D001+D002)"
      :summary "精核 traits.rs 13 traits, 给出 10 项 trait 级对齐动作 (rename 1 + delete 1 + merge 5 + 新建 2 + 保留 N)"
      :resolved ["D001" "D002" "I003"]
      :at "2026-04-20")

    (comp-003
      :phase "phase-1.6-gen-migration-sqlite-investigation"
      :agent "explore-agent (dispatched for I004 I005 I006)"
      :summary "两套 gen_*.rs 都可删 (未被 mod.rs 导入); migration.rs 是 SQLite schema 脚本非 runner; sqlite/ 13 文件仍 active 为迁移工具, 分步清理"
      :resolved ["I005" "I006" "I007 info"]
      :refined ["I004"]
      :at "2026-04-20"
      :phase-2-ready true)

    (comp-004
      :phase "phase-2-stage-2A.1"
      :agent "general-purpose agent (adf12f667fb1b1875)"
      :summary "删除两套 gen_*.rs: db/gen_*.rs 根下 8 个 + db/gen/ 子目录 8 个 = 16 文件"
      :deleted "16 文件 / 8302 LOC / ~285KB"
      :verified-mod-rs "无 mod gen_* / 无 pub mod gen; 确认未导入"
      :cargo-build "通过 (Finished dev profile in 12.93s, 仅 pg/board.rs:852 unused_mut pre-existing warning)"
      :resolved ["I005"]
      :at "2026-04-20")

    (comp-005
      :phase "phase-2-stage-2A.2"
      :agent "general-purpose agent (a7c3bc9b9e93c7bd5)"
      :summary "KnowledgeStore → KbStore rename (代码对齐 lisp)"
      :files-modified 3
      :cargo-build "workspace 通过"
      :at "2026-04-20")

    (comp-006
      :phase "phase-2-stage-2A.3"
      :agent "主 Claude (直接执行, 基于 general-purpose agent a108680e474c84b1e 调研)"
      :summary "TimelineStore 归属修正 + 删 db/timeline.rs 空壳"
      :key-finding "D003: TimelineStore 不是 deprecated 而是 pillar 四 projection 读接口, 归 pillar 四"
      :changes
        ("删除 db/timeline.rs (551B re-export 冗余)"
         "删除 db/mod.rs:46 sqlite-gated 'pub(crate) mod timeline;'")
      :untouched "TimelineStore trait / pg/timeline.rs / sqlite/timeline.rs (归 pillar 四)"
      :cargo-build "通过"
      :at "2026-04-20")

    (comp-007 :phase "2B.1" :summary "InfraStore trait 空壳" :cargo "通过" :at "2026-04-20")
    (comp-008 :phase "2B.2" :agent "a29e205bbc09f4a16" :summary "DirectiveLayerStore 17 方法完整 PG impl (types+db+pg 3 新文件)" :cargo "通过" :at "2026-04-20")
    (comp-009 :phase "2C.1" :agent "a1ceaa5a700e618e1" :summary "SkillStore 25 方法合并进 ProjectStore (+8=33 total); pg/project.rs 的 8 方法搬进 pg/skill.rs 单一 impl; 补建 db/project.rs (18L re-export + D001 达成)" :cargo-workspace "通过" :rust-analyzer-note "E0119/E0046 IDE 误报, cargo feature 组合不同导致" :at "2026-04-20")
    (comp-010 :phase "2C.2+2C.3+2C.4" :agent "aaeb025f65c3e3bb0" :summary "ToolCallStore(19)+EventStore(7)+RetrospectiveStore(15) 合并进 ConversationStore; 删 pg/{tool_call,event,retrospective}.rs + sqlite/同名 6 文件" :cargo-workspace "通过" :at "2026-04-20")
    (comp-011 :phase "2C.5" :agent "a2f09b18427fb6ad3" :summary "VisionStore 10 方法合并进 ObservabilityStore; 补建 db/observability.rs" :cargo "通过" :at "2026-04-20")
    (comp-012 :phase "2D" :agent "af46ad1392fe21100" :summary "InfraStore 填 24 方法" :cargo "通过" :at "2026-04-20")
    (comp-013 :phase "2E" :agent "af7519bb19abb5f42"
      :summary "SQLite 生态一次性大扫除 — 删 31 文件 ~14018 LOC ~360KB"
      :details "sqlite/ 整目录 + db/ 下 21 sqlite-gated 文件 + 5 个 src/ 文件重写去 sqlite 分支 + Cargo.toml 清理"
      :migrate-tool-preserved "pg/migrate_from_sqlite.rs 保留 feature-gated 'all(sqlite, postgres)' 当前永不编译 (DEPRECATED note)"
      :db-mod-rs "422→58 行"
      :cargo "--workspace + --tests --no-run 0 error"
      :at "2026-04-20"))

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
      :resolved "phase-1.5 agent 确认 60 表现存 + 1 已 drop (system_timeline) = 61 migrations 定义; 对齐 lisp ✓"
      :at "2026-04-20 phase-1 scan")

    (I004
      :severity "high"
      :desc "db/sqlite/ 子目录存在 13 个文件 (deprecated per lisp). 需清理"
      :refined-at-phase-1.6 "13 文件 (非 15), mod.rs 无 deprecated 标记但注释说 'Wraps MissionDB via DbExecutor'. 仍 active 用于 SQLite → PG 数据迁移 (通过 pg/migrate_from_sqlite.rs)"
      :still-active-reason "cfg(feature = 'sqlite') 编译门控; migrate_from_sqlite 仍依赖"
      :resolution-path "(分步) phase-2 里: 验证迁移完成 → 删 cfg(feature='sqlite') 代码 → 最后删整个 sqlite/ 目录"
      :at "2026-04-20 phase-1.5 ls 发现, phase-1.6 精查")

    (I005
      :severity "high"
      :desc "gen_*.rs 两套: db/ 根下 8 个 + db/gen/ 子目录 8 个"
      :resolved-at-phase-1.6
        :两套关系 "完全重复 (文件名对应, 内容相同). 都标 // GENERATED BY FORGE - DO NOT EDIT, Source: intent-db.lisp"
        :mtime "根目录 2026-04-19 (新), db/gen/ 2026-04-09 (旧缓存)"
        :active-status "⚠ 两套都未被 mod.rs 导入 — 当前均不活跃, 被 traits.rs + pg/ 架构取代"
        :action "phase-2 删除两套共 16 个 gen_*.rs 文件 (约 250KB)"
      :at "2026-04-20 phase-1.5 ls → phase-1.6 resolved")

    (I006
      :severity "medium"
      :desc "migration.rs (73KB) 在 db/ 根下, 性质未知"
      :resolved-at-phase-1.6
        :实际是 "SQLite 表创建 SCHEMA 脚本 (const SCHEMA + init() 函数, 1675 行)"
        :非 "migration runner (PG 用 sqlx::migrate! + migrations/*.sql)"
        :依赖方 "仅 SQLite backend 启动时调用"
        :action "phase-2 里: 标 [deprecated]; 跟 sqlite/ 清理同步删除"
      :at "2026-04-20 phase-1.5 ls → phase-1.6 resolved")

    (I007
      :severity "info"
      :desc "traits.rs 架构确认"
      :findings
        ("traits.rs 行首注释说 '12 domain-specific async traits' (实际 13, 行首注释有 1 数字误)"
         "最后 20 行定义 super-trait MissionStore 聚合所有 13 个 domain traits + init() 方法"
         "SQLite 和 PG backend 都实现 MissionStore (traits.rs 作为统一入口设计正确)")
      :施工-note "MissionStore super-trait 架构和 lisp cross-cutting :: capability db-trait-abstraction 一致; v0.4.20 已修正 stores 数字 9 → 实际 13 (代码向 lisp 对齐后 9 primary + sub-traits 合并)"
      :at "2026-04-20 phase-1.6"))

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
