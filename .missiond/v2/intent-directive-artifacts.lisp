;; ═════════════════════════════════════════════════════════════
;; MissionD — Directive Artifacts Shard (L2 split, wave 15 task 02)
;; 目标: file-first artifacts 注册 + 5 artifact 详细字段 + writer integration 锚
;;       review-gate policy + id derivation + write_file/review_gate args
;; 来源: 由 wave 15 task 02 从 intent-memory.lisp :: module directive-layer 物理 split
;;       内容与原主 Lisp byte-identical (no-content-mutation, R5)
;; 父引用: intent-memory.lisp 留 stub 指回本 shard
;; 拆分计划: architecture-dsl.lisp :: l2-shard-split-plan ::
;;           shard intent-directive-artifacts
;; 备注: review-gate-policy / review-gate-id-derivation /
;;       directive-write-file-args / plan-write-file-args /
;;       workflow-write-file-args / review-gate-args / file-first-writer-integration
;;       是 source-index-only section-id (无主 Lisp 子块); 它们的 :source-file
;;       重定向到本 shard 后, anchor 指向本文件的 narrative meta 块
;; ═════════════════════════════════════════════════════════════

(directive-artifacts-shard
  :version "v1"
  :origin "wave 15 task 02 — physical split from memory/intent-layer/tools parents"
  :status "code-aligned partial (per source-index entries); writer integration 主路径已接入 (wave 14 task 01); review-gate auto-create v1 已落 (wave 14 task 03)"
  :stability "section-ids preserved (R008 + R016); content byte-identical to pre-split parents"

  ;; ──────────────────────────────────────────────────────────
  ;; A · memory pillar :: module directive-layer :: file-first-artifacts
  ;; (was: intent-memory.lisp lines 1764–1843)
  ;; section-id roots:
  ;;   memory.directive-layer.file-first-artifacts
  ;;   memory.directive-layer.plan-evidence-sidecar (artifact)
  ;;   memory.directive-layer.plan-node-state-projection (artifact)
  ;; ──────────────────────────────────────────────────────────
    (file-first-artifacts
      :status "code-aligned partial — sexp/DB row 写入由 actor v0 (directive-compiler / plan-compiler / workflow-distiller / methodology compiler / plan-runner); file-first .lisp 自动同步仍 pending — 当前由 ClaudeCode/外部 LLM 手工产出文件"
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
        :purpose "plan 执行证据落盘 — git diff / tests / tool_calls refs / event_log refs / execution companion log refs / deviations / decisions / completions / plan_runner_dispatch entries / plan_dag_node_dispatch entries"
        :writer "evidence-collector role — mission_plan(record_evidence) 显式 + plan-runner v0 internal mode 自动追加 plan_runner_dispatch entry + plan_dag runtime v2 每节点 dispatch/state transition 写 plan_dag_node_dispatch entry (含 6 lifecycle + 3 skip 子分类 + skip_reason/skip_detail)"
        :writer-typed "typed EvidenceEntry helper (handlers/knowledge/evidence_collector.rs) — canonical source/kind/schema_version + with_extra flat-top byte-for-byte 兼容 legacy passthrough; plan.rs + plan_dag.rs 已接入 (anchor: intent-layer.evidence-collector-typed-helper)"
        :status "code-aligned partial — sidecar 写入 + typed helper 接入 + legacy passthrough 兼容; 全自动 evidence-collector actor (跨执行路径) + live ExecutionEvent ref (EventRef::unavailable 占位中) + plan_evidence DB JSONB 仍 pending"
        :consumer "workflow-distiller (s8) + retrospective + capability-usage-monitor (间接)"
        :ssot-note "当前文件即真相; 升级为 DB 时保留文件作为长期归档"
        :dag-scheduler-future "actor plan-dag-scheduler 完整 11-stage 升级时, sidecar 增 nodes[<node_id>] block (per-node attempts / claim_id / start-end / inner_result / acceptance_pass / rollback_path); v0 plan_runner_dispatch + runtime v2 plan_dag_node_dispatch entry shape 必须向后兼容")

      (artifact plan-node-state-projection
        :path ".missiond/v2/plans/<plan_id>.evidence.json :: nodes[<node_id>] (sidecar 子树, 与 plan-evidence-sidecar 同文件)"
        :status code-aligned-partial
        :wave-13-task-02-progress "plan_dag runtime v2 (commit 8bb6110): 已写顶层 plan_dag_node_dispatch entry, 6 lifecycle (pending/ready/running/succeeded/failed/skipped) + 3 skip 子分类 (skipped_upstream_failed + failed_dep / skipped_condition / skipped_fail_fast_abort + aborter); 每节点 dispatch + state transition 串行化避免文件 race; nodes[<node_id>] 子树形态仍 pending — 待完整 11-stage scheduler claim-lease/retry/rollback/acceptance 落地 (anchor: intent-layer.plan-dag-runtime-v2.node-lifecycle)"
        :purpose "完整 PLAN DAG scheduler 的 per-node 状态 + evidence 聚合视图 — 当前 v0 单节点 dispatch 不需要; runtime v2 已写顶层 entry; 完整 11-stage 启用后增 nodes[<node_id>] 子树"
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

  ;; ──────────────────────────────────────────────────────────
  ;; B · file-first-writer-integration anchor
  ;; (was: source-index-only section-id, no separate body in parent)
  ;; section-id: memory.directive-layer.file-first-writer-integration
  ;; ──────────────────────────────────────────────────────────
  (file-first-writer-integration
    :status "code-aligned (wave 14 task 01 commit 00cbc1d)"
    :scope "三类 artifact (directive alignment / PLAN.lisp / workflow methodology) 主路径"
    :writer-helper "file_artifacts::attempt_artifact_write → resolve_target_project_root → atomic_write_artifact"
    :no-fallback "process cwd fallback 严禁 — 必须显式 project|cwd|target_project, 否则 fail-fast"
    :partial-semantics "DB 已写但 file 失败 → status=partial + file_write_error (不回滚 DB row, 不静默吞错)"
    :response-fields ["file_written" "file_path" "file_sha256" "file_bytes" "file_created" "file_overwritten"]
    :implementation-targets
      ["crates/missiond-daemon/src/handlers/knowledge/file_artifacts.rs"
       "crates/missiond-daemon/src/handlers/knowledge/directive.rs"
       "crates/missiond-daemon/src/handlers/knowledge/plan.rs"
       "crates/missiond-daemon/src/handlers/knowledge/workflow.rs"
       "crates/missiond-mcp/src/tools/knowledge/directive.rs"
       "crates/missiond-mcp/src/tools/knowledge/plan.rs"
       "crates/missiond-mcp/src/tools/knowledge/workflow.rs"]
    :cross-ref ["memory.directive-layer.file-first-artifacts"
                "intent-layer.unified-entry-pipeline.file-first-ssot"
                "flow.file-vs-db-contract"
                "intent-layer.unified-entry-pipeline.run-pipeline-helper"]
    :note "writer 主路径升 code-aligned, schema/contract 段不压缩")

  ;; ──────────────────────────────────────────────────────────
  ;; C · review-gate policy + id derivation anchors
  ;; (was: source-index-only section-ids, no separate body in parent)
  ;; section-ids:
  ;;   intent-layer.unified-entry-pipeline.review-gate-policy
  ;;   intent-layer.unified-entry-pipeline.review-gate-id-derivation
  ;; ──────────────────────────────────────────────────────────
  (review-gate-policy
    :status "code-aligned (wave 14 task 03 commit 96842cd)"
    :enum [manual emit_question off]
    :default manual
    :default-rule "byte-identical legacy: default 不破"
    :emit_question-behavior "artifact 写入成功后自动 emit QuestionEvent::Created"
    :off-behavior "禁用 review gate, 不 emit 任何 question"
    :ownership "review_gate.rs (handlers/knowledge/review_gate.rs)"
    :no-auto-approve "policy=emit_question 不等人答, 不自动 approve (4 项 v0 non-goal 仍生效)"
    :implementation-targets
      ["crates/missiond-daemon/src/handlers/knowledge/review_gate.rs"
       "crates/missiond-daemon/src/handlers/knowledge/directive.rs"
       "crates/missiond-daemon/src/handlers/knowledge/plan.rs"
       "crates/missiond-daemon/src/handlers/knowledge/workflow.rs"
       "crates/missiond-mcp/src/tools/knowledge/directive.rs"
       "crates/missiond-mcp/src/tools/knowledge/plan.rs"
       "crates/missiond-mcp/src/tools/knowledge/workflow.rs"]
    :cross-ref ["intent-layer.unified-entry-pipeline.alignment-review-gate"
                "intent-layer.unified-entry-pipeline.plan-review-gate"
                "intent-layer.unified-entry-pipeline.review-gate-id-derivation"
                "intent-layer.unified-entry-pipeline.run-pipeline-helper"])

  (review-gate-id-derivation
    :status "code-aligned (wave 14 task 03 commit 96842cd)"
    :format "review:<scope>:<id>:v<v>:<action>[:<topic-hash>]"
    :topic-hash-rule "SHA-256 前 16 hex; file_path 优先, 否则 topic"
    :file-write-failure-rule "file 写失败时拒发 question + warning (review_gate_policy=emit_question requires file_written=true)"
    :resolve-actions ["approve" "archive" "mark" "supersede"]
    :resolve-rule "review_question_id resolution 继续支持; 4 actions 接 review_question_id 参数"
    :implementation-targets ["crates/missiond-daemon/src/handlers/knowledge/review_gate.rs"]
    :cross-ref ["intent-layer.unified-entry-pipeline.review-gate-policy"
                "intent-layer.unified-entry-pipeline.alignment-review-gate"
                "intent-layer.unified-entry-pipeline.plan-review-gate"])

  ;; ──────────────────────────────────────────────────────────
  ;; D · tools args anchors (write_file + review_gate args)
  ;; (was: source-index-only section-ids, no separate body in parent)
  ;; section-ids:
  ;;   tools.surface.directive-write-file-args
  ;;   tools.surface.plan-write-file-args
  ;;   tools.surface.workflow-write-file-args
  ;;   tools.surface.review-gate-args
  ;; ──────────────────────────────────────────────────────────
  (directive-write-file-args
    :status "code-aligned (wave 14 task 01 commit 00cbc1d)"
    :scope "mission_directive(action=compile)"
    :args ["write_file" "topic" "overwrite_file" "project" "cwd" "target_project"]
    :write_file-rule "write_file=true 必须搭配 topic"
    :resolution-order "project (registered id) 优先 → cwd (绝对路径) → target_project 兜底"
    :response-fields-6 ["file_written" "file_path" "file_sha256" "file_bytes" "file_created" "file_overwritten"]
    :partial-semantics "DB 已写, file 失败 → status=partial + file_write_error"
    :implementation-targets
      ["crates/missiond-mcp/src/tools/knowledge/directive.rs"
       "crates/missiond-daemon/src/handlers/knowledge/directive.rs"
       "crates/missiond-daemon/src/handlers/knowledge/file_artifacts.rs"]
    :cross-ref ["memory.directive-layer.file-first-writer-integration"
                "tools.surface.plan-write-file-args"
                "tools.surface.workflow-write-file-args"])

  (plan-write-file-args
    :status "code-aligned (wave 14 task 01 commit 00cbc1d)"
    :scope "mission_plan(action=compile)"
    :args ["write_file" "topic" "overwrite_file" "project" "cwd" "target_project"]
    :file-path-rule "PLAN.lisp 写入 <project_root>/.missiond/plans/<topic>/PLAN.lisp"
    :topic-fallback "topic 默认走 board_task_id 兜底"
    :response-fields-6 ["file_written" "file_path" "file_sha256" "file_bytes" "file_created" "file_overwritten"]
    :partial-semantics "DB 已写, file 失败 → status=partial + file_write_error"
    :implementation-targets
      ["crates/missiond-mcp/src/tools/knowledge/plan.rs"
       "crates/missiond-daemon/src/handlers/knowledge/plan.rs"
       "crates/missiond-daemon/src/handlers/knowledge/file_artifacts.rs"]
    :cross-ref ["memory.directive-layer.file-first-writer-integration"
                "tools.surface.directive-write-file-args"
                "tools.surface.workflow-write-file-args"])

  (workflow-write-file-args
    :status "code-aligned (wave 14 task 01 commit 00cbc1d)"
    :scope "mission_workflow(action=distill|compile_methodology)"
    :args ["write_file" "topic|name" "overwrite_file" "project"]
    :file-path-rule "workflow .lisp 写入 <project_root>/.missiond/workflows/<topic>.lisp"
    :artifact-kind "ArtifactKind::Workflow"
    :actions-supported ["distill" "compile_methodology"]
    :response-fields-6 ["file_written" "file_path" "file_sha256" "file_bytes" "file_created" "file_overwritten"]
    :implementation-targets
      ["crates/missiond-mcp/src/tools/knowledge/workflow.rs"
       "crates/missiond-daemon/src/handlers/knowledge/workflow.rs"
       "crates/missiond-daemon/src/handlers/knowledge/file_artifacts.rs"]
    :cross-ref ["memory.directive-layer.file-first-writer-integration"
                "tools.surface.directive-write-file-args"
                "tools.surface.plan-write-file-args"])

  (review-gate-args
    :status "code-aligned (wave 14 task 03 commit 96842cd)"
    :scope "mission_directive/plan/workflow"
    :new-args ["review_gate_policy" "emit_review_question (legacy bool, 兼容)" "review_question_text" "review_question_id"]
    :policy-enum [manual emit_question off]
    :response-fields-4 ["review_question_emitted" "review_question_id" "review_gate_policy" "review_question_warning"]
    :tool-count-invariant "tool count 仍 83 不变"
    :implementation-targets
      ["crates/missiond-mcp/src/tools/knowledge/directive.rs"
       "crates/missiond-mcp/src/tools/knowledge/plan.rs"
       "crates/missiond-mcp/src/tools/knowledge/workflow.rs"
       "crates/missiond-daemon/src/handlers/knowledge/review_gate.rs"]
    :cross-ref ["intent-layer.unified-entry-pipeline.review-gate-policy"
                "intent-layer.unified-entry-pipeline.review-gate-id-derivation"]))
