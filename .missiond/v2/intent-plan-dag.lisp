;; ═════════════════════════════════════════════════════════════
;; MissionD — PLAN DAG Shard (L2 split, wave 15 task 02)
;; 目标: PLAN DAG scheduler / runtime v2 / 节点 schema / FSM / failure-policy /
;;       live event ref / open-questions / dag-scheduler narrative
;; 来源: 由 wave 15 task 02 从 intent-intent-layer.lisp + intent-flow.lisp 物理 split;
;;       内容与原主 Lisp byte-identical (no-content-mutation, R5)
;; 父引用: intent-intent-layer.lisp + intent-flow.lisp 各留 stub 指回本 shard
;; 拆分计划: architecture-dsl.lisp :: l2-shard-split-plan :: shard intent-plan-dag
;; 备注: PlanNodeStateChanged variant 仍在 event-bus.lisp (frozen, 不进 shard)
;; ═════════════════════════════════════════════════════════════

(plan-dag-shard
  :version "v1"
  :origin "wave 15 task 02 — physical split from intent-layer + flow parents"
  :status "code-aligned partial — runtime v2 已落 (wave 13 task 02); wave 15 task 05 (commit 615b249) 加 workstation-dispatch v0 opt-in — plan_dag.rs::dispatch_node 接 :workstation-dispatch true 节点路由到 workstation_dispatch.rs; 经 mission_task_delegate; 完整 11-stage (claim-lease/retry/rollback/acceptance/review-gate paused/mark-plan-final/distill 触发) 仍 architecture-designed pending (anchor: intent-layer.actor.plan-dag-scheduler / intent-layer.unified-entry-pipeline.workstation-dispatch-v0)"
  :stability "section-ids preserved (R008 + R016); content byte-identical to pre-split parents"

  ;; ──────────────────────────────────────────────────────────
  ;; A · intent-layer pillar :: section action-instruction-actor ::
  ;;     actor plan-dag-scheduler (含 minimal-node-schema + node FSM +
  ;;     claim-lease-protocol + 11-stage logic-core + open-design-questions)
  ;; (was: intent-intent-layer.lisp lines 1138–1231)
  ;; section-id roots:
  ;;   intent-layer.actor.plan-dag-scheduler
  ;;   intent-layer.plan-dag-runtime-v2
  ;;   intent-layer.plan-dag-runtime-v2.node-lifecycle
  ;;   intent-layer.plan-dag-runtime-v2.failure-policy
  ;;   intent-layer.plan-dag-runtime-v2.execution-event-decision
  ;;   intent-layer.plan-dag-runtime-v2.live-event-ref-strategy
  ;; ──────────────────────────────────────────────────────────
    (actor plan-dag-scheduler
      :status code-aligned-partial
      :desc "完整 PLAN DAG scheduler — plan-runner v1 升级目标; v0 单节点 + runtime v2 wave-based concurrency 已落; v1 完整 11-stage 引入调度循环 / claim-lease / retry-failure-policy / condition-gate / rollback-compensation / acceptance evaluator / review-gate paused / mark-plan-final / trigger-record-execution-distill"
      :v0-relation "复用 v0 sexp hint parser + dispatch_strategy 注入 + evidence sidecar + companion log meta + agent-team idempotent; runtime v2 升级到 wave-based concurrency; v1 升级到 plan-runner 持 DAG + ready-set + 节点 FSM + claim 生命周期 (复用 agent-execution-coordination)"
      :runtime-v2-status "wave 13 task 02 commit 8bb6110 — plan_dag.rs: max_parallel_nodes (default=1=v1 sequential) / tokio::JoinSet wave-based / 每 wave drain up to max_parallel_nodes / failure-policy fail-fast (停后续 wave, 未 drain 标 skipped_fail_fast_abort + aborter) vs continue (失败子树标 skipped_upstream_failed + failed_dep) / per-node typed EvidenceEntry 串行化 / dry_run 返 DAG + concurrency_plan / response 含 scheduler_mode/node_count/max_parallel_nodes/node_results[]/skipped_nodes[]/aggregate_status; +17 tests; ExecutionEvent 不扩 (wave 13 task 02 决议) — anchor: intent-layer.plan-dag-runtime-v2"
      :code-aligned-current ["crates/missiond-daemon/src/handlers/knowledge/plan_dag.rs (runtime v2)"
                              "crates/missiond-daemon/src/handlers/knowledge/plan.rs (action=execute internal v0 + 多节点 → plan_dag runtime v2)"
                              "crates/missiond-daemon/src/handlers/knowledge/evidence_collector.rs (typed EvidenceEntry 接入)"
                              "crates/missiond-mcp/src/tools/knowledge/plan.rs (schema: max_parallel_nodes)"]
      :future-target ["crates/missiond-daemon/src/intent_layer/plan_runner.rs (新文件 — 完整 11-stage scheduler 抽离 + claim-lease/retry/rollback)"
                      "crates/missiond-daemon/src/intent_layer/plan_dag.rs (新文件 — 完整 DAG/cycle 检测/ready-set/节点 FSM enum 10 状态; runtime v2 是 6 主 + 3 skip 子集)"]
      :input "approved plan (sexp_text 含 (nodes …) (edges …)) + plan-level / 节点级 hints"
      :output "plan FSM final (succeeded/failed/succeeded-partial) + per-node evidence sidecar + execution companion log claims/completions/issues/deviations slots"
      :flow-cross-ref "flow pillar :: F-intent-alignment-plan-execution-loop :: s6 execution-runner :: dag-scheduler — 完整 11 stage 协议正文"
      :coordination-protocol-cross-ref "memory pillar :: module board :: helper agent-execution-coordination — id-counters / claims-with-lease / audit-repair / derived-indexes 必须复用 (D010 教训)"

      (minimal-node-schema
        :scope "PLAN.lisp 节点字段; plan-compiler v1 写入 / file-first writer 写入 / 人工 review 编辑都按本 schema"
        :required-fields
          ((id              :type "string (kebab-case)" :desc "DAG 内唯一稳定标识, 重 run 不变")
           (objective       :type "string"              :desc "节点意图, 传给 dispatch substrate / 任务 .md")
           (target          :type "enum"                :desc "∈ {mission_execution / mission_task_delegate / mission_flow_run}"))
        :optional-fields
          ((title            :type "string"              :desc "人读标题 (review gate 用)")
           (flow-id          :type "string"              :desc "若 target=mission_flow_run 必填, plan-compiler 产出的 generated flow_id")
           (dispatch-strategy :type "enum"               :desc "∈ {resident-lisp / fresh-code-alignment / agent-team / mixed / prompt-fallback / unknown}; 缺省继承 plan-level")
           (parallelism     :type "int"                 :desc "节点内 sub-task 并发 hint")
           (target-project  :type "string"              :desc "节点级目标项目, 缺省继承 plan-level")
           (requested-cwd   :type "path"                :desc "覆盖 spawn cwd, 必须在 target_project_root 内")
           (depends-on      :type "list[node-id]"       :desc "DAG 边; 空 = 根节点")
           (condition       :type "sexp predicate"      :desc "gate 表达式, 引用上游节点 evidence; 不满足 → skipped (不算 failed)")
           (failure-policy  :type "enum + meta"         :desc "∈ {fail-fast / retry-N(:max-attempts :backoff-ms) / continue-on-failure / route-to-rollback}; 缺省继承 plan-level (默认 fail-fast)")
           (rollback-policy :type "enum + ref"          :desc "∈ {none / compensate-node(:ref node-id) / cascade-up}; 缺省 none")
           (timeout-ms      :type "int"                 :desc "节点 wall-clock 上限; 触发即 timeout, 走 failure-policy")
           (evidence-tags   :type "list[string]"        :desc "节点级 evidence 分类 hint, evidence-collector 按 tag 聚合")
           (acceptance      :type "sexp"                :desc "节点完成判据 (cmd / kb_query / file_exists 等); 缺省 = inner result success")
           (review-gate     :type "enum"                :desc "∈ {none / question-event / human-checkpoint}; 触发 plan paused, 等 QuestionEvent::Resolved"))
        :node-fsm-enum
          [pending ready claimed running succeeded failed skipped retrying rolling-back paused]
        :validation-rules
          ["DAG 必须无 cycle — s1 load-plan-graph 检测, 失败 plan FSM → failed"
           "depends-on 必须全部解析为已存在的 node-id"
           "rollback-policy=compensate-node 的 :ref 必须指向同 plan 内已声明的节点"
           "requested-cwd (若有) 必须在 target_project_root 之内"
           "节点 FSM 转移必须经 scheduler — 外部不允许直接写节点状态"])

      (claim-lease-protocol-binding
        :rule "scheduler 必须经 mission_execution(action=claim) 申请节点 scope=plan/<plan_id>/node/<node_id> claim_id, 不允许自建 ID 池"
        :lease "lease_secs 由节点 :failure-policy / :timeout-ms 派生; heartbeat 由 dispatch substrate 内部循环维护"
        :reap "lease 过期未 heartbeat → mission_execution reap, scheduler 把节点回 ready (s4)"
        :anti-pattern "scheduler 自己管 claim_id / lease 是 D010 教训直接复发 — 必须从 helper agent-execution-coordination manager 拿"
        :cross-ref "memory pillar :: module board :: helper agent-execution-coordination :: shared-memory-slots :: id-counters / claims")

      (logic-core
        (step s1 "load-plan-graph: 解析 (nodes …)(edges …), 构建 DAG, cycle 检测")
        (step s2 "validate-node-schema: 校验必填字段 / enum 值 / depends-on 引用 / failure-policy 形状")
        (step s3 "resolve-target-project-root: 节点级 > plan 级 > directive 级; 校验 :requested-cwd 在 root 内")
        (step s4 "build-ready-set: 计算 {n | depends-on 全 succeeded/skipped ∧ condition 通过 ∧ 当前并发 < limit}")
        (step s5 "acquire-execution-claim: mission_execution(action=claim, scope=plan/<plan_id>/node/<node_id>); manager 原子分配 claim_id + lease")
        (step s6 "dispatch-ready-nodes: 路由 mission_execution / mission_task_delegate / mission_flow_run; companion log meta + evidence sidecar plan_runner_dispatch (per node)")
        (step s7 "collect-node-evidence: 聚合 inner result + companion log slice + acceptance 验证, 落 sidecar nodes[<node_id>] block")
        (step s8 "update-node-state: running → succeeded/failed/timeout; succeeded 触发 release claim + 重算 ready-set")
        (step s9 "handle-retry-failure-rollback: 按 failure-policy 分支 (fail-fast / retry-N / continue-on-failure / route-to-rollback) + rollback-policy 触发 compensate")
        (step s10 "mark-plan-final: 全节点终态 → plan FSM 推 succeeded / failed / succeeded-partial via mission_plan(action=mark)")
        (step s11 "trigger-record-execution-distill-candidate: mission_workflow(action=record_execution); 满足条件 enqueue distill 给 s8 workflow-distillation"))

      (egress
        :writes ["plan FSM final state (executing → succeeded | failed | succeeded-partial) via DirectiveLayerStore::plan_update_status"
                 ".missiond/v2/plans/<plan_id>.evidence.json :: nodes[<node_id>] block (per-node attempts/claim_id/start-end/inner_result/acceptance_pass/rollback_path)"
                 "mission_execution companion log claims/completions/deviations/issues slots (per-node)"
                 "scheduler tick observability hint (capability_usage)"]
        :emits ["future ExecutionEvent::PlanNodeStateChanged (含 plan_id / node_id / from / to / dispatch_strategy / target_project; 当前 ExecutionEvent dispatch metadata 仍 pending)"
                "future PlanCompleted / PlanFailed / PlanPartial DomainEvent (经 mission_plan(action=mark) 触发)"]
        :returns "plan_id + final_status + per-node summary + distill_candidate_enqueued flag"
        :downstream-flow ["F-intent-alignment-plan-execution-loop :: s7 evidence-collection (sidecar 升级)"
                          "F-intent-alignment-plan-execution-loop :: s8 workflow-distillation (distill 候选消费)"
                          "F-execution-log-governance (per-node companion log audit/repair)"
                          "F-capability-usage-monitoring (dispatch_strategy 命中率 / 节点 retry 模式)"])

      (anti-patterns
        ["不允许 scheduler 自建 claim/lease — agent-execution-coordination 是单一真相 (D010 教训)"
         "不允许 silent retry — 每次 retry 写 evidence + 增 attempt counter; 失败信息暴露不掩盖"
         "不允许节点失败时静默吞错 — failure-policy 必须显式声明; 缺省 fail-fast"
         "不允许 client 自行解析 PLAN.lisp DAG 后逐节点重新调用 mission_plan(execute) — scheduler 必须在 daemon 内"
         "不允许把 'continue-on-failure' 当默认 — 那是兜底反模式"
         "不允许把 prompt-fallback 当 dispatch_strategy 默认 — 默认 fail-fast 走 spawn 路径"])

      (open-design-questions
        ["dispatch substrate 间的 cross-node lock — 多节点共享 resident-lisp slot 时, 是 scheduler 持锁还是 mission_execution claim 子 scope?"
         "rollback semantic 是否可全自动 — 当前默认 :rollback-policy=none, 是否引入 'auto-compensate by reverse-dispatch' 待审"
         "DAG-level vs node-level :failure-policy 优先级 — 节点级覆盖 plan 级是合理默认"
         "scheduler tick 周期 — 事件驱动 (ExecutionEvent::PlanNodeStateChanged) 还是固定 tick? 当前推荐事件驱动 + autopilot fallback tick"
         "节点级 review-gate=question-event 触发后, plan FSM 是 paused 还是新增 paused 态?"
         "节点 :acceptance sexp 的 evaluator — 复用 deterministic methodology compiler 的 sexp 子集还是新做"]))

  ;; ──────────────────────────────────────────────────────────
  ;; B · flow pillar :: F-intent-alignment-plan-execution-loop ::
  ;;     s6 execution-runner :: (dag-scheduler ...) sub-block
  ;; (was: intent-flow.lisp lines 1169–1351)
  ;; section-id root: flow.execution-runner-dag-scheduler
  ;; ──────────────────────────────────────────────────────────
            (dag-scheduler
              :status code-aligned-partial
              :runtime-v2-status "wave 13 task 02 (commit 8bb6110) — plan_dag.rs runtime v2: max_parallel_nodes / tokio::JoinSet / 6 lifecycle + 3 skip 子分类 (upstream_failed/fail_fast_aborted/condition_gated) / failure-policy fail-fast vs continue / per-node typed EvidenceEntry 串行化; 完整 11-stage (claim-lease / per-node retry / rollback / acceptance / review-gate paused / mark-plan-final) 仍 pending"
              :coverage "完整 PLAN DAG scheduler — 多节点 dependency / 并发 dispatch / per-node retry-failure-policy / condition-gate / rollback-compensation / per-node evidence aggregation"
              :scope "本 sub-section 不实现, 只声明 ingress → 11-stage logic-core → egress 契约, 给后续 plan-runner v1+ 代码同构作 anchor"
              :v0-relation "复用 v0 + auto-selection v1 的 sexp hint parser / dispatch_strategy 注入 / evidence sidecar / companion log meta; 升级是 'mission_plan(action=execute) 单节点一次性 dispatch' → 'plan-runner 持有调度循环 + claim/lease + ready-set + 节点 FSM'"
              :anti-pattern "不允许 client 自行解析 PLAN.lisp DAG 后逐节点重新调用 mission_plan(execute) — 那等于把 scheduler 下沉给某个 client 私有循环, 违反 unified-pipeline 原则"
              :execution-protocol-cross-ref "memory pillar :: module board :: helper agent-execution-coordination — id-counters / claims-with-lease / audit-repair / derived-indexes 必须复用, 不再发明新 atomic 协议"
              :ingress
                ((trigger "mission_plan(action=execute, execute_mode=internal) 接收 approved plan_id (PLAN.lisp DAG 含 ≥2 节点)")
                 (state  "plan row status=approved (review gate 已收敛)")
                 (state  "PLAN.lisp 文件 / plan.sexp_text 含 (nodes …) DAG 与 (edges …) / 节点 :depends-on 字段")
                 (hints  "target_project / requested_cwd / dispatch_strategy / parallelism / failure_policy / rollback_policy 默认值 (节点级可覆盖)")
                 (existing-evidence ".missiond/v2/plans/<plan_id>.evidence.json (v0 plan_runner_dispatch entry — DAG run 必须续写, 不覆盖)"))
              :node-schema-minimal
                ((field :id              :type "string (kebab-case)" :required t  :role "节点稳定标识 — DAG 内唯一, 重 run 不变")
                 (field :title           :type "string"              :required nil :role "人读标题 (review gate 用)")
                 (field :objective       :type "string"              :required t  :role "节点意图 (传给 dispatch substrate / task .md)")
                 (field :target          :type "enum"                :required t  :role "执行 substrate ∈ {mission_execution / mission_task_delegate / mission_flow_run}")
                 (field :flow-id         :type "string"              :required nil :role "若 target=mission_flow_run 必填")
                 (field :dispatch-strategy :type "enum"              :required nil :role "∈ {resident-lisp / fresh-code-alignment / agent-team / mixed / prompt-fallback / unknown}; 缺省继承 plan-level 默认")
                 (field :parallelism     :type "int"                 :required nil :role "节点内并发 sub-task 数 hint; 与 DAG 间并发独立")
                 (field :target-project  :type "string"              :required nil :role "节点级目标项目 — 缺省继承 plan-level; project-root cwd 契约消费")
                 (field :requested-cwd   :type "path"                :required nil :role "覆盖 spawn cwd; 必须在 target_project_root 之内")
                 (field :depends-on      :type "list[node-id]"       :required nil :role "DAG 边 — ready-set 计算输入; 空 = 根节点")
                 (field :condition       :type "sexp predicate"      :required nil :role "gate 表达式 — 引用上游节点 evidence; 不满足则 skipped (不算 failed)")
                 (field :failure-policy  :type "enum + meta"         :required nil :role "∈ {fail-fast / retry-N / continue-on-failure / route-to-rollback}; retry 必须含 :max-attempts + :backoff-ms")
                 (field :rollback-policy :type "enum + ref"          :required nil :role "∈ {none / compensate-node / cascade-up}; compensate-node 引用同 plan 内 :id 的补偿节点")
                 (field :timeout-ms      :type "int"                 :required nil :role "节点 wall-clock 上限; 触发即 timeout 状态, 走 failure-policy")
                 (field :evidence-tags   :type "list[string]"        :required nil :role "节点级 evidence 分类 hint, evidence-collector 落 sidecar 时按 tag 聚合")
                 (field :acceptance      :type "sexp"                :required nil :role "节点完成判据 (cmd / kb_query / file_exists 等); 缺省 = inner result success/failure")
                 (field :review-gate     :type "enum"                :required nil :role "∈ {none / question-event / human-checkpoint}; 触发时 plan 推 paused, 等 QuestionEvent 解锁"))
              :node-state-fsm
                ((state pending      :enter "DAG 加载, depends-on 未全部 succeeded/skipped")
                 (state ready        :enter "depends-on 满足 + condition 通过 (或缺省)")
                 (state claimed      :enter "scheduler 取走 ready 节点, 已分配 claim_id + lease_expires_at (复用 agent-execution-coordination claims slot)")
                 (state running      :enter "已 dispatch 到 substrate, 等 inner result")
                 (state succeeded    :enter "inner result OK + acceptance 通过")
                 (state failed       :enter "inner result error 或 acceptance 拒绝, 失败已不可重试 (failure-policy 耗尽)")
                 (state skipped      :enter "condition 不通过, 不算失败")
                 (state retrying     :enter "失败后处于 backoff 等待重新进 ready (retry counter < max-attempts)")
                 (state rolling-back :enter "下游失败触发 rollback, 当前节点正在执行 compensate")
                 (state paused       :enter "review-gate=question-event 触发, 等 QuestionEvent::Resolved"))
              :logic-core
                ((stage s1 load-plan-graph
                    :at "intent-layer pillar :: plan-runner v1 (architecture-designed)"
                    :reads ["plan row (status=approved)" "plan.sexp_text" ".missiond/plans/<topic>/PLAN.lisp 当前是 DB 镜像源"]
                    :action "解析 (nodes …) (edges …) 与节点字段; 构建有向图, 检测 cycle (cycle = 拒绝执行, plan FSM → failed + 写 issue)"
                    :writes ["scheduler 内存图 (plan_id → DAG)" "evidence sidecar entry plan_runner_graph_loaded"]
                    :failure-mode "cycle / 未知节点 id / depends-on 引用孤儿 → 立刻 reject, 不进入下一阶段; plan 不允许部分 load")
                 (stage s2 validate-node-schema
                    :at "intent-layer pillar :: plan-runner v1"
                    :reads ["per-node 字段"]
                    :action "校验必填字段 (id / objective / target); 校验 enum 值; 校验 :depends-on 全部存在; failure-policy/rollback-policy 形状校验"
                    :writes ["evidence sidecar entry plan_runner_schema_validated (per node ok / err)"]
                    :failure-mode "任一节点校验失败 → reject 整张 DAG, 不退化到 'partial run'; 错误明确指向节点 id")
                 (stage s3 resolve-target-project-root
                    :at "intent-layer pillar :: plan-runner v1 → memory pillar :: project-management :: project-registry"
                    :reads ["plan-level :target_project / 节点级 :target-project / :requested-cwd / ProjectRegistry"]
                    :action "为每节点解析 target_project_root: 节点级 > plan 级 > directive 级; 校验 :requested-cwd 必须在 root 之内; 复用 worker pillar :: section pty :: invariant project-root-spawn-cwd"
                    :writes ["per-node resolved_project_root + resolved_cwd"]
                    :failure-mode "解析失败或 cwd 越界 → 节点 schema check fail, 整图 reject; 不允许 fallback 到 missiond 仓根")
                 (stage s4 build-ready-set
                    :at "intent-layer pillar :: plan-runner v1"
                    :reads ["DAG + 当前节点 FSM 状态 + parallelism 限制 (plan-level / dispatch_strategy 派生)"]
                    :action "计算 ready-set = {n | n.state=pending ∧ ∀d∈n.depends-on (d.state ∈ {succeeded, skipped}) ∧ n.condition 通过 ∧ 当前并发 < limit}; condition 不通过的节点直接 → skipped (递归向下推)"
                    :writes ["scheduler 内存 ready-set (cache, 不写文件)"]
                    :idempotency "ready-set 必须从 durable 节点 FSM 重建; scheduler crash 后下次 tick 仍能恢复同一 ready-set")
                 (stage s5 acquire-execution-claim
                    :at "intent-layer pillar :: plan-runner v1 → memory pillar :: helper agent-execution-coordination"
                    :reads ["mission_execution 协议 claims slot / id-counters"]
                    :action "为 ready 节点调 mission_execution(action=claim, scope=plan/<plan_id>/node/<node_id>, lease_secs=<from policy>); claim_id manager 原子分配; lease 过期未 heartbeat → reap, 节点回 ready"
                    :writes ["execution companion log claims slot + active_claims index"]
                    :guarantee "scope overlap 必拒绝 — 同一节点不允许两 scheduler 实例同时 claim; 复用 D010 教训的 manager 原子分配 (intent-event-bus / intent-memory execution lisp 已证实)"
                    :failure-mode "claim 失败 (overlap / lease store 不可用) → 节点保持 ready, 下 tick 重试; scheduler 不允许 'best-effort dispatch 不 claim'")
                 (stage s6 dispatch-ready-nodes
                    :at "intent-layer pillar :: plan-runner v1 → tools pillar :: mission_execution / mission_task_delegate / mission_flow_run"
                    :action "已 claim 节点按 :target 路由: mission_execution(open) → spawn/复用 ClaudeCode slot (companion log meta dispatch_strategy/target_project/requested_cwd 写入, 复用 v0 路径); mission_task_delegate → 已存在 slot 挂任务; mission_flow_run → flow-engine-v2 runner; agent-team hint 注入复用 v0 idempotent 逻辑"
                    :writes ["mission_execution / mission_task_delegate / mission_flow_run 入参 + companion log meta"
                             "evidence sidecar entry plan_runner_dispatch (per node, 含 claim_id / dispatch_strategy / target_project / requested_cwd / inner_result_handle)"
                             "节点 FSM: claimed → running"]
                    :concurrency "ready-set 内多节点可并发 dispatch (受 plan/dispatch-strategy 派生 parallelism 限制); 节点之间共享 substrate (resident-lisp slot) 必须串行化, scheduler 持锁"
                    :anti-pattern "不允许直接调 mission_pty_spawn 绕过 mission_execution — execution log 是协调真相")
                 (stage s7 collect-node-evidence
                    :at "intent-layer pillar :: evidence-collector role + memory pillar :: agent-execution-coordination"
                    :reads ["dispatch substrate 回包 (mission_execution status / inner result)" "ExecutionEvent stream (dispatch metadata 扩展后, 当前仅 companion log)" "tool_calls 从 conversation_messages 派生 (per node 范围)" "git diff (cwd-scoped)" "test outputs (节点 acceptance 调用)"]
                    :action "按节点 :evidence-tags 聚合 inner result + companion log slice + acceptance 验证结果, 落 evidence sidecar 子条目 plan_runner_node_evidence (per node)"
                    :writes [".missiond/v2/plans/<plan_id>.evidence.json :: nodes[<node_id>] block (含 attempts[] / claim_id / start/end / inner_result / acceptance_pass / artifacts_refs)"
                             "execution companion log :: completions slot (mission_execution(action=complete, phase=node-<node_id>))"]
                    :ssot-rule "evidence sidecar 是 file-first 真相; 升级到 plan_evidence DB JSONB 时保留 sidecar 作为长期归档 (与 v0 一致)")
                 (stage s8 update-node-state
                    :at "intent-layer pillar :: plan-runner v1"
                    :reads ["acceptance 验证结果 + inner result + timeout 检测 + heartbeat 生命周期"]
                    :action "节点 FSM 推进: running → succeeded | failed | timeout; succeeded 触发 release claim + 重算 ready-set (s4); failed 转 stage s9 走 failure-policy"
                    :writes ["scheduler 内存节点 FSM" "execution companion log claims slot release (mission_execution(action=release))"
                             "evidence sidecar entry plan_runner_node_state_change"]
                    :idempotency "节点 FSM 必须从 evidence + companion log 可重建 — scheduler crash 不丢状态")
                 (stage s9 handle-retry-failure-rollback
                    :at "intent-layer pillar :: plan-runner v1"
                    :reads ["节点 :failure-policy / :rollback-policy / 当前 attempt count / 上下游节点 FSM"]
                    :action
                      ((branch retry-N
                          :rule "attempt < max-attempts → 节点回 retrying, backoff-ms 后重新进 ready (s4); 写 evidence sidecar attempt 子条目 + 增加 id-counter")
                       (branch fail-fast
                          :rule "节点 → failed; scheduler 立即 mark plan failed (s10); 取消所有 ready/claimed/running 节点 (release claim + cancel inner)")
                       (branch continue-on-failure
                          :rule "节点 → failed; 不影响下游 ready-set 计算 (下游可视为 :depends-on satisfied=skipped); 仅记入 issue slot (mission_execution(action=issue))")
                       (branch route-to-rollback
                          :rule "节点 → failed; rollback-policy=compensate-node 触发对应补偿节点入 ready-set; rollback-policy=cascade-up 触发上游已 succeeded 节点反向 dispatch 补偿"))
                    :writes ["节点 FSM 转移" "execution companion log issues / deviations / decisions slot (按分支语义)"]
                    :anti-pattern "不允许 silent retry — 每次 retry 必须写 evidence + 增 attempt counter; 不允许默默吞错 (fail-fast 原则)")
                 (stage s10 mark-plan-final
                    :at "intent-layer pillar :: plan-runner v1 → memory pillar :: directive-layer plan FSM"
                    :reads ["全节点 FSM"]
                    :action
                      ((rule plan-succeeded
                          :condition "所有节点 ∈ {succeeded, skipped}"
                          :writes "plan FSM: executing → succeeded; mission_plan(action=mark, status=succeeded); 触发 record_evidence 闭环 (s11)")
                       (rule plan-failed
                          :condition "存在节点 failed 且 failure-policy=fail-fast 或 rollback 终态 failed"
                          :writes "plan FSM: executing → failed; companion log meta failure_summary; 标记 unresolved_issues")
                       (rule plan-partial
                          :condition "存在 failed 节点但 failure-policy=continue-on-failure, 其余 succeeded"
                          :writes "plan FSM: executing → succeeded (with partial flag in evidence sidecar); review_required=true; mission_plan(action=mark, status=succeeded, evidence_partial=true)"))
                    :failure-mode "plan FSM 写入失败 (DB 不可用) → 暴露 partial, 不假装成功; sidecar 保留所有节点真相方便 audit/repair")
                 (stage s11 trigger-record-execution-distill-candidate
                    :at "intent-layer pillar :: plan-runner v1 → tools pillar :: mission_workflow"
                    :reads ["plan FSM 终态 + evidence sidecar 全文"]
                    :action
                      ((path success
                          :writes "mission_workflow(action=record_execution) 累计 success_count + avg_cost_usd; 满足触发条件 (success_count ≥ 阈值 或 human pin) 时 enqueue distill candidate 事件 (供 s8 workflow-distillation 消费)")
                       (path failure
                          :writes "mission_workflow(action=record_execution, success=false); failure pattern 入 capability_usage hints 用于后续 negative-rule 学习"))
                    :downstream-flow "F-intent-alignment-plan-execution-loop :: s7 evidence-collection 收口 + s8 workflow-distillation 触发"))
              :egress
                ((writes
                    ["plan FSM final state (executing → succeeded | failed | succeeded-partial) via mission_plan(action=mark) — DirectiveLayerStore::plan_update_status"
                     "evidence sidecar nodes[<node_id>] block (per-node aggregate; attempts[] 历史; rollback path)"
                     "mission_execution companion log claims/completions/deviations/issues slots (per-node)"
                     "scheduler ticks 计数 (capability_usage observability hint)"])
                 (emits
                    ["future ExecutionEvent::PlanNodeStateChanged (含 plan_id / node_id / from / to / dispatch_strategy / target_project) — 当前 ExecutionEvent dispatch metadata 仍 pending"
                     "future PlanCompleted / PlanFailed / PlanPartial DomainEvent — 通过 mission_plan(action=mark) 触发 plan FSM event"])
                 (returns
                    "plan_id + final_status + per-node summary (id/state/attempts/dispatch_strategy/elapsed_ms) + distill_candidate_enqueued flag")
                 (downstream
                    ["F-intent-alignment-plan-execution-loop :: s7 evidence-collection (sidecar 升级)"
                     "F-intent-alignment-plan-execution-loop :: s8 workflow-distillation (distill candidate 消费)"
                     "F-execution-log-governance (per-node companion log audit/repair)"
                     "F-capability-usage-monitoring (dispatch_strategy 命中率 / 节点 retry 模式)"
                     "F-workstation-dispatch-policy (策略命中后回写 ExecutionEvent)"]))
              :file-vs-db-contract
                ("plan.sexp_text + .missiond/plans/<topic>/PLAN.lisp 是 DAG 真相 (file-first SSOT) — file-first writer 落地后, 文件即真相; 当前 plan.sexp_text 是 DB 镜像源"
                 "evidence sidecar (.missiond/v2/plans/<plan_id>.evidence.json) 是节点级证据真相 — 升级到 plan_evidence DB JSONB 时保留 sidecar"
                 "execution companion log 是 claim/lease/id-counter 真相 — 不允许 scheduler 自建 ID 池 (违反 agent-execution-coordination D010 教训)"
                 "scheduler 内存 DAG / ready-set / 节点 FSM 是 cache, 必须可从 sidecar + companion log 重建")
              :anti-patterns
                ["不允许 scheduler 自建 claim/lease 协议绕过 mission_execution — agent-execution-coordination 是单一真相"
                 "不允许 silent retry — 每次重试写 evidence + 增 attempt counter, 失败信息暴露不掩盖"
                 "不允许节点失败时静默吞错 — failure-policy 必须显式声明; 缺省 fail-fast"
                 "不允许 client 自行解析 PLAN.lisp DAG 后逐节点重新调用 mission_plan(execute) — scheduler 必须在 daemon 内"
                 "不允许把 'continue-on-failure' 当默认 — 那等于鼓励兜底; 必须 PLAN.lisp 节点显式声明"
                 "不允许把 prompt-fallback 当 dispatch_strategy 默认 — 默认 fail-fast 走 spawn 路径"]
              :open-design-questions
                ["dispatch substrate 间的 cross-node lock — 多节点共享 resident-lisp slot 时, 是 scheduler 持锁还是 mission_execution claim 子 scope?"
                 "rollback semantic 是否可全自动 — 当前默认 :rollback-policy=none, 需要节点显式声明 compensate-node; 是否引入 'auto-compensate by reverse-dispatch' 仍待审"
                 "DAG-level vs node-level :failure-policy 优先级 — 节点级覆盖 plan 级是合理默认, 但需文档化"
                 "scheduler tick 周期 — 事件驱动 (ExecutionEvent::PlanNodeStateChanged) 还是固定 tick? 当前推荐事件驱动 + autopilot fallback tick"
                 "节点级 review-gate=question-event 触发后, plan FSM 是 paused 还是 awaiting_approval? 复用既有 plan FSM 还是新增 paused 态?"]
              :implementation-targets-current
                ["crates/missiond-daemon/src/handlers/knowledge/plan_dag.rs (runtime v2; 详 :runtime-v2-status)"
                 "crates/missiond-daemon/src/handlers/knowledge/plan.rs (action=execute internal v0 — 单节点 dispatch + auto-selection v1; 多节点 → plan_dag runtime v2)"
                 "crates/missiond-daemon/src/handlers/knowledge/evidence_collector.rs (typed EvidenceEntry helper; EventRef::unavailable 占位)"
                 "crates/missiond-daemon/src/handlers/knowledge/agent_execution.rs (claim/heartbeat/release 接入)"
                 "crates/missiond-core/src/db/pg/directive.rs (plan_update_status)"]
              :implementation-targets-future
                ["crates/missiond-daemon/src/intent_layer/plan_runner.rs (新文件 — 提取完整 11-stage scheduler, claim-lease/retry/rollback; runtime v2 仅覆盖 dispatch + lifecycle + failure-policy fail-fast/continue 子集)"
                 "crates/missiond-core/src/event/events/execution.rs (ExecutionEvent::PlanNodeStateChanged 扩展; dispatch metadata pending — 见 anchor intent-layer.plan-dag-runtime-v2.execution-event-decision)"
                 "crates/missiond-core/src/db/pg/directive.rs (新增 plan_evidence JSONB 列待审)"]
              :checker-contract
                ("scheduler 实现完成必须保持 evidence sidecar shape 向后兼容 — v0 plan_runner_dispatch entry 仍可读"
                 "scheduler 实现完成必须保持 mission_execution claims/completions slot shape — agent-execution-coordination 协议不动"
                 "scheduler 实现完成必须保持 mission_plan(execute) 单节点 fast-path — 无 DAG 时退化为 v0 行为")))
