;; ═════════════════════════════════════════════════════════════
;; MissionD — Workstation Policy Shard (L2 split, wave 15 task 02)
;; 目标: ClaudeCode 工位调度的策略层 — 何时复用常驻 Lisp / 何时新开代码同构 / 何时
;;       agent-team / 为何优先 spawn / 项目根 cwd 契约 / scoped commit handoff
;; 来源: 由 wave 15 task 02 从 intent-worker.lisp + intent-flow.lisp +
;;       intent-intent-layer.lisp 物理 split 出独立 shard,
;;       内容与原主 Lisp byte-identical (no-content-mutation, R5)
;; 父引用: intent-worker.lisp / intent-flow.lisp / intent-intent-layer.lisp 各留 stub
;;         指回本 shard
;; 拆分计划: architecture-dsl.lisp :: l2-shard-split-plan :: shard intent-workstation-policy
;; ═════════════════════════════════════════════════════════════

(workstation-policy-shard
  :version "v1"
  :origin "wave 15 task 02 — physical split from worker/flow/intent-layer parents"
  :status "operational-practice + code-aligned (per source-index entries); wave 15-18 持续闭环: workstation-dispatch v0 + auto-inference v1 + scoped commit handoff daemon enforce v0 + scoped-commit handoff brief default v1 + PLAN DAG claim-lease v0 + autonomous PLAN field inference v0 + scoped commit worktree preflight v0. wave 19 task 07 (commit bfc72b7) 加 workstation task-contract consumer v0 — parse_task_contract / load_task_contract / WorkstationDispatchHints::overlay_contract; SafeDescriptorReason::MalformedTaskContract; legacy brief byte-identical 当 contract absent; **绝不 fall back claude -p**; anchor: intent-layer.unified-entry-pipeline.workstation-task-contract-consumer-v0. wave 19 task 08 (commit 405d13b) 加 execution task-contract completion verification v0 — mission_execution(complete) 加 4 字段 + 2 新错误码 + claim scope ⊇ contract write-scope; daemon 仍只 read-only; anchor: intent-layer.unified-entry-pipeline.task-contract-completion-verification-v0. **wave 20 task 03 (commit fe835e8) 加 execution preflight task-contract scope v1** — mission_execution preflight_commit 加 8 新字段 (task_contract_status / staged_out_of_scope / staged_forbidden / unstaged_in_scope / task_contract_scope / next_step / task_contract_error / task_contract_resolved_path) 把 contract 对账下沉进 daemon; 无新错误码 (信息性 surface); **grep proof 0 mutating git** (仍只 git status --porcelain=v1); legacy byte-compat 当 task_contract_path absent; anchor: intent-layer.scoped-commit-worktree-preflight-task-contract-v1. **wave 20 task 04 (commit 681c95d) 加 machine-driven dispatch v0** — DispatchContractMode { Rendered (default), Machine } 接通 wave-19/07 dormant consumer; **Lisp 真正成 dispatch SSOT, Markdown brief 不再 load-bearing**; 默认 Rendered 不破 wave-15..19 byte-shape; anchor: intent-layer.machine-contract.dispatch-machine-mode-v0. **wave 20 task 09 (commit 6e01e3f) 加 ExecutionEvent legacy metadata sweep v0** — 8 legacy variants (Heartbeat / Released / DeviationRecorded / DecisionRecorded / IssueRecorded / Audited / Repaired / StaleClaim) 全加 dispatch_strategy/target_project/requested_cwd; 11 variants 全闭环, 不再有 dispatch metadata gap; 从 companion log read_dispatch_metadata_from_log 自动继承; anchor: event-bus.section.execution-event.dispatch-metadata-legacy-sweep-v0. **brief → preflight (含 contract scope wave 20-03) → staged guard (wave 20-02) → commit → verifier (wave 19-02) → execution complete verify (wave 19-08, wave 21-03 daemon-internal verified gate 升强) 六段闭环**. **wave 21 task 04 (commit 68b84f1) 加 autonomous workstation LLM proposal v0** — workstation_inference_mode=off|sonnet_suggest opt-in; WorkstationProposalGate 仅在 caller_target/dispatch_strategy/objective/scope/owned_files/project_signal AND plan_hints + plan_workstation_opt_in 全空时才 propose 4 字段 (target/dispatch_strategy/objective/scope) × 3 confidence × 4 safety status (Safe/InvalidTarget/InvalidStrategy/UnsupportedTarget); workstation_proposals[] cap 6; **propose only never auto-spawn — applied=false / auto_spawn=false 永钉死**; conservative whitelists target ∈ {mission_execution, mission_task_delegate, mission_flow_run} + dispatch_strategy ∈ {resident-lisp, fresh-code-alignment, agent-team, mixed} (prompt-fallback / unknown 排除); Sonnet unavailable surfaces status='llm_unavailable' + reason 钉 'no fallback to claude -p / prompt mode'; gate-suppressed surfaces signal_summary; DAG mode preflight rejects sonnet_suggest INVALID_PARAM; attach_workstation_proposals_block 在每 dispatch 分支 (executing / dispatch_skipped / dry_run / inner_error / safe-descriptor / bridge) splice 不污染 errors; anchor: intent-layer.workstation.autonomous-workstation-llm-proposal-v0. **wave 21 task 03 (commit 308426e) 加 execution report-verifier integration v1** — mission_execution(complete) 加 4 新字段 (task_run_verifier_status / shared_memory_path / verifier_diagnostics / verified) + verified=true 触发 daemon-internal sexp-parse cross-check (无新 dep) + 4 structured error codes (VERIFIED_REQUIRES_ENFORCEMENT/TASK_CONTRACT/TASK_REPORT/COMMIT_HASH + TASK_REPORT_REQUIRED/MALFORMED + TASK_REPORT_TASK_ID_MISMATCH + TASK_REPORT_COMMIT_HASH_MISMATCH + TASK_CONTRACT_MALFORMED); preflight_commit echoes task_report_path/shared_memory_path advisory hints; 4 新字段持久化为 Lisp keyword 进 completion entry; daemon NEVER spawns Node — wave21-02 verifier 仍 out-of-process 权威; daemon_never_invokes_mutating_git unit-pinned; anchor: intent-layer.unified-entry-pipeline.execution-report-verifier-integration-v1. **wave 22 task 05 (commit 162a303) 加 autonomous workstation true spawn v1** — auto_spawn=true + workstation_caller_approved=true + preflight_acceptable=true + workstation_proposal_hash 4 opt-in apply gate (mode sonnet_suggest 是前置必填); **12-rule gate matrix** G1-G12: G1 auto_spawn opt-in / G2 bundle Suggested / G3 hash matches (compute_workstation_proposal_hash 32-hex SHA-256 over v1 sentinel + bundle status + each proposal field|value|confidence|safety_status semicolon-joined, evidence text excluded) / G4 ALL proposals safety_status=safe / G5 ALL proposals confidence=high / G6 caller_approved=true / G7 preflight_acceptable=true / G8 task_contract_path supplied / G9 contract loads ok (resolve_contract_path_public + load_task_contract pre-load) / G10 :write-scope non-empty / G11 :write-scope non-overlap with :must-not-touch / G12 proposed target=mission_task_delegate; 走 `mission_task_delegate` substrate **绝不 `claude -p`** (substrate's existing SafeDescriptorReason::UnsupportedTarget 拒非 mission_task_delegate up-front, AND G12 在 substrate 之前也拒, 双层防御); 3 新错误码 AUTO_SPAWN_INVALID_PARAM / AUTO_SPAWN_MISSING_PROPOSAL_HASH / AUTO_SPAWN_PROPOSAL_HASH_MISMATCH (hash mismatch fail-fast BEFORE substrate dispatch); 15 status taxonomy (not_requested|spawned|skipped_unavailable|skipped_no_proposals|skipped_unsafe_proposal|skipped_confidence_too_low|skipped_caller_not_approved|skipped_missing_task_contract_path|skipped_malformed_task_contract|skipped_empty_write_scope|skipped_forbidden_scope_overlap|skipped_preflight_unacceptable|skipped_unsupported_target|skipped_substrate_refused|skipped_substrate_inner_error); attach_workstation_auto_spawn_gate_block 在 successful responses splice (preserves pre-existing blocks, skips error results); **wave-21/04 4 invariants 全 preserved 4 dedicated tests** (default off / Sonnet unavailable no fallback / DAG mode rejects / propose-only fields preserved on wave-21/04 surface — wave-22/05 surface 在 SEPARATE workstation_auto_spawn_gate block); anchor: intent-layer.workstation.autonomous-workstation-true-spawn-v1. **wave 22 task 02 (commit 02ac627) 加 execution auto-run-verifier v2** — daemon-internal `auto_run_task_run_verifier` 8 cross-checks in-process (task_contract_loadable / task_report_loadable / task_report_schema / task_id_matches_contract / commit_hash_matches_report / shared_memory_loadable / shared_memory_schema / shared_memory_completion_for_task); 当 task_contract_path + task_report_path + shared_memory_path + commit_hash 4 路径全提供时 daemon 自跑 cross-check; verification_source='daemon-auto-verifier' 主路径; legacy verified=true with missing paths degrade verification_source='legacy-caller-claim' verifier_status=unknown 不硬拒 (back-compat); 3 新错误码 SHARED_MEMORY_REQUIRED / SHARED_MEMORY_MALFORMED / SHARED_MEMORY_NO_COMPLETION_FOR_TASK; daemon NEVER spawns Node — wave21-02 verifier 仍 out-of-process 权威; daemon-internal 用 in-tree sexp parser; anchor: intent-layer.unified-entry-pipeline.execution-auto-run-verifier-v2. **brief → preflight (含 contract scope wave 20-03) → staged guard (wave 20-02) → commit → verifier (post-commit out-of-process) → execution complete verify (daemon-internal **auto_run_task_run_verifier** 4 路径全提供主路径 'daemon-auto-verifier' wave-22/02 强化) 六段闭环**. 完全 autonomous spawn 无任何 PLAN hint 无 task_contract_path (wave22-05 仍要求 task_contract_path + 12-rule gate; 完全无 hint 全局 LLM 推断 spawn 仍 future) + 后续 enforce-by-default + git config core.hooksPath .githooks default-on real install (wave22-01 仍 doctor only) + LLM auto-approve 真正落 review row state transition (wave22-03 已加 review LLM approve apply gate v1, 6 道严格 gate + apply_llm_auto_approve + proposal_hash + caller_approved 3 strict opt-in, 但仍要 caller 显式 opt-in) 仍 surface 不实现"
  :stability "section-ids preserved (R008 + R016); content byte-identical to pre-split parents"

  ;; ──────────────────────────────────────────────────────────
  ;; A · worker pillar :: section claudecode-workstation-orchestration
  ;; (was: intent-worker.lisp lines 1954–2106)
  ;; section-id roots:
  ;;   worker.section.claudecode-workstation-orchestration
  ;;   worker.section.claudecode-workstation-orchestration.dispatch-decision-matrix
  ;;   worker.section.claudecode-workstation-orchestration.execution-strategy-record
  ;; ──────────────────────────────────────────────────────────
  (section claudecode-workstation-orchestration
    :desc "MissionD 调度 ClaudeCode 工作时的默认策略 — 何时复用常驻 Lisp 会话 / 何时新开代码同构会话 / 何时提示 agent-team / 为何优先 spawn 工位 / 项目根 cwd 契约 / 如何 scoped commit"
    :status "operational-practice (人工流程已在用) + architecture-designed (Lisp 已声明); shared execution log + scoped commit handoff 设计完成; auto-selection by plan-runner code-alignment pending"
    :rationale "用户提的 5 条真实经验需要从'临时使用技巧'升级为 MissionD plan-runner / worker orchestration 的默认调度规则, 不再依赖某次会话的临时记忆"
    :scope "本 section 描述 worker pillar 视角的调度策略 — 决策语义同时挂在 intent-layer pillar :: section unified-entry-pipeline :: workstation-dispatch-policy"
    :flow-cross-ref "flow pillar :: F-workstation-dispatch-policy + F-intent-alignment-plan-execution-loop :: s2 / s4 / s6"
    :intent-layer-cross-ref "intent-layer pillar :: section unified-entry-pipeline :: workstation-dispatch-policy"

    (policy resident-lisp-architect-session
      :purpose "持续改 .missiond/v2/*.lisp / 维护架构上下文 / 减少重新定位成本"
      :preferred-dispatch "复用已有常驻 ClaudeCode slot/session (PTY slot 长期保持, 接 mission_pty_send / mission_task_delegate; 不为每次 Lisp 改动重开会话)"
      :substrate-tools ["mission_pty_spawn (initial spawn — sole-spawn-bottleneck)"
                        "mission_pty_send / mission_pty_read (持续对话)"
                        "mission_task_delegate (用既有 slot 执行新任务)"
                        "mission_compute_slot (若需要新建 dynamic slot 角色为 lisp-architect)"]
      :context-asset "已加载的 v2/*.lisp 文件结构、checker 行为、最近一次 alignment topic; 重新定位成本几乎为零"
      :risk ["上下文可能漂移 — 长期会话中早期错误判断可能被后续累积引用"
             "每次新任务仍必须读对应 .missiond/claudecode/<topic>.md 任务文件"
             "每次写完仍必须运行 node scripts/check-architecture-lisp.mjs --all-v2 校验"]
      :status "operational-practice; spawn 与 send 已是 code-aligned tool; 自动'识别本次任务属于 Lisp 改动并路由到常驻 slot'还需要 plan-runner 实现"
      :anti-pattern "不要为每次 Lisp 修改重开 fresh ClaudeCode 会话 — 会丢失架构上下文,变成纯 .md 重读")

    (policy fresh-code-alignment-session
      :purpose "代码同构任务 — Rust / SQL / JS 修改 / 跨多 crate 重构"
      :preferred-dispatch "新开 ClaudeCode 工位 (mission_pty_spawn 新 slot; 或 mission_compute_slot create dynamic slot)"
      :substrate-tools ["mission_pty_spawn (新 slot, project-root cwd)"
                        "mission_compute_slot (create dynamic slot for code-alignment role)"
                        "mission_task_delegate (把任务挂载到新 slot)"]
      :why-fresh ["任务 .md 文件自包含 — scope / no-goals / acceptance / files / risks 全部写清楚"
                  "隔离度更好 — 不被前序 Lisp 设计讨论污染"
                  "可并发多个独立代码同构任务 (multi-slot fan-out)"
                  "崩溃 / 上下文炸了 / 中途换主导 agent 都不影响其它 slot"]
      :requirement ".missiond/claudecode/<topic>.md 必须 self-contained — 任务范围、禁止事项、验收命令、文件列表"
      :status "operational-practice; spawn 与 task_delegate 已 code-aligned; 自动'识别本次任务属于 code-alignment 并 spawn 新 slot' 待 plan-runner 实现"
      :risk "若任务 .md 不够自包含, fresh slot 缺乏上下文会反复返工 — 写 .md 的成本由 alignment-author 承担")

    (policy agent-team-hint
      :purpose "复杂任务可拆分为多个独立读/查/验证子任务 或 跨多个 pillar 定位时的并行加速"
      :trigger-conditions
        ["任务可拆成 N≥2 个独立可并行的读/查/扫描子任务"
         "需要在多个 pillar (memory / worker / flow / intent-layer / tools) 同时定位 anchor"
         "需要从多个文件并行读取或验证, 而最终汇总写入由单点完成"]
      :directive "在给 ClaudeCode 的任务 .md 中加入'使用 agent-team 提高效率'文字; ClaudeCode 主会话即会并发开 Explore / general-purpose 子 agent"
      :guardrail
        [(read-side "并行子 agent 可以读和给建议 — 不限并发数, 但避免重复搜索同一 anchor")
         (write-side "最终写入必须由主 agent 串行落笔 — 严禁多个子 agent 并发写同一 Lisp 块, 会造成结构漂移")
         (file-claim "如必须多 agent 写, 用 mission_execution claims 字段绑定 file scope, 显式 lease + heartbeat")
         (review-still-required "agent-team 不替代 review gate — alignment / plan 仍要走 unified-entry-pipeline 的 alignment-review-gate 与 plan-review-gate")]
      :status "operational-practice — 当前由 ClaudeCode 主会话识别 agent-team 提示后调用 Agent tool 并发派 sub-agent; MissionD 自身的 plan-runner 自动加 hint 待实现"
      :code-alignment-pending "plan-runner 根据 PLAN.lisp 节点的 :parallelism 标签自动在派工 .md 里添加 agent-team 提示"
      :anti-pattern "不要为简单单文件修改触发 agent-team — 只会增加 token 成本, 不加速")

    (policy spawn-over-prompt-mode
      :purpose "默认通过 MissionD spawn / resident workstation 调度 ClaudeCode, 尽量少用 `claude -p` 一次性 prompt"
      :preferred-dispatch "mission_pty_spawn (常驻 slot) 或 mission_compute_slot (动态 slot) — 进入 sole-spawn-bottleneck → spawner.rs::spawn_tracked_slot"
      :why-spawn-wins
        ["spawn 工位有显式 cwd / project_root (project-root-spawn-cwd invariant)"
         "spawn 工位有 MCP config 注入 (perm-injector + mcp_config 字段)"
         "spawn 工位的 stdout/stderr 进 PTY session, 可被 mission_pty_read / mission_pty_screenshot / extractor pipeline 持续观测"
         "spawn 工位的 tool_calls 进 conversation_tool_calls 表, 自动纳入 capability-usage-monitor 与 retrospective"
         "spawn 工位的执行可挂入 mission_execution 12-action 协调面 (claim/heartbeat/release + ExecutionEvent)"
         "spawn 工位的产物自动落 evidence sidecar (.missiond/v2/plans/<plan_id>.evidence.json)"
         "session_id 稳定 — JSONL ingest / conversation-logger / retrospective 可线性追溯"]
      :why-prompt-mode-loses
        ["`claude -p` 一次性 prompt 没有持久 session_id, conversation 进不了 conversation-logger pipeline"
         "无 PTY session, extractor / anomaly / screenshot 全部失效"
         "无 cwd 治理 — project-root-spawn-cwd 不强制"
         "无法纳入 mission_execution / mission_capability_usage / workflow-distillation 闭环"
         "崩溃恢复不可观测 — supervisor / ControlTree 看不到此进程"]
      :exception "仅在以下情况退到 prompt-mode: (1) MissionD daemon 不可用时的人工应急; (2) 真正一次性、无需观测、无需 evidence 的 throwaway 查询; (3) CI/CD 脚本里 ClaudeCode 一行命令"
      :status "operational-practice; spawn-side 已 code-aligned; '禁止 plan-runner 默认走 -p' 是架构规则, 不是代码 enforce — code-alignment pending"
      :enforcement-future "plan-runner 默认通过 spawn 路径; 显式 dispatch_strategy=prompt-fallback 才允许 -p; 否则 fail-fast")

    (policy project-root-cwd-contract
      :purpose "fresh code-alignment 工位的 cwd 必须落在目标项目根 — 与 sole-spawn-bottleneck 的 project-root-spawn-cwd invariant 是同一规则的两端表述"
      :cross-ref "section pty :: invariant project-root-spawn-cwd (line 516) + worker pillar :: core-6b"
      :resident-lisp-session-rule "常驻 Lisp 架构会话的 cwd 通常是 missiond 仓库根 (因为本仓 .missiond/v2/*.lisp 是工作面)"
      :fresh-code-alignment-session-rule "fresh 代码同构会话必须 spawn 在目标项目 root (例: 改 cuthub Rust → spawn cwd = ~/Projects/cuthub; 改 missiond Rust → spawn cwd = ~/Projects/missiond)"
      :validation "spawner::spawn_tracked_slot 已强制 cwd 解析; 跨项目复用 slot 必须 slot.project_root == target_project_root, 否则另开 slot"
      :status "code-aligned for spawn cwd enforcement; 任务 .md 中显式声明 :target_project 字段, 让 plan-runner 决定 reuse 还是 new spawn — 这一段的自动选路待实现"
      :anti-pattern "不要把 missiond 常驻 Lisp 会话用来执行 cuthub 的 Rust 修改 — cwd 不对, project memory / JSONL / tool path 全错位")

    (policy scoped-commit-handoff
      :purpose "并行工位完成 claim scope 后, 用 git commit 固化成果, 并把 commit_hash 回填 execution log"
      :why "共享 execution Lisp 是 control plane, 可协调 claim/lease/decision; 但未提交代码仍可能被后续 agent 的 stash/pop/Edit 回退. scoped commit 是 durability plane."
      :cross-ref "memory pillar :: helper agent-execution-coordination :: scoped-commit-contract + flow pillar :: F-scoped-commit-handoff"
      :applies-to
        [(code-alignment-new-session "required — Rust/SQL/JS/TS 代码同构完成并验收后必须只提交自己 scope")
         (parallel-code-agent "required — 并行写代码风险最高, 每个 agent 独立 commit 自己 ownership")
         (resident-lisp-architect "wave-boundary-or-required — 常驻 Lisp 会话可以按 wave 汇总 commit, 但每个 wave 必须落 durable hash")
         (exploration-only "not-required — 只读调查写 completion 即可")
         (emergency-fix "required-after-verify — 修正任务必须可回滚")]
      :handoff-steps
        [(s1 "mission_execution(action=claim) 先锁定文件/模块 scope")
         (s2 "工作期间 heartbeat; 发现越界写入需求则 issue/deviate, 不抢写")
         (s3 "验收通过后 mission_execution(action=complete) 写 changed_files + verification")
         (s4 "git add 仅添加 claimed_scope 内文件; 禁止 git add .")
         (s5 "git diff --cached --name-only 必须是 claimed_scope 子集")
         (s6 "git commit scoped patch")
         (s7 "commit_hash 回填 completion/git-handoff; 若失败写 commit_status=blocked + blocker")]
      :task-file-contract "所有给 ClaudeCode 的写入型任务 .md 必须包含本 handoff policy; 若任务禁止 commit, 必须写明替代 durability artifact (patch file / branch / worktree)"
      :status "code-aligned partial — wave 16 task 06 (commit 591d288) daemon enforce v0 opt-in default false; wave 17 task 07 (commit a21b8ae) brief-layer default v1 — workstation_dispatch.rs::build_task_brief 任务 brief section 7 'Completion handoff (scoped commit)' 强制新派工 worker 走 enforce_scoped_commit=true / commit_status / commit_hash / staged_files; daemon legacy mission_execution(action=complete) 仍 enforce_scoped_commit default false 字节兼容; daemon 自动 preflight/stage/commit + git pre-commit hook 执行 scope check 仍 pending"
      :anti-patterns ["不要 git add ."
                      "不要把别人的 unstaged 文件带进 commit"
                      "不要用 stash/reset/checkout 清理别人的 scope"
                      "不要只看测试通过就宣称交付 — 必须确认 live diff 或 commit hash 存在"])

    (dispatch-decision-matrix
      :desc "plan-runner 选择策略的决策表 — 当前由人/上层 actor 手动应用; 未来由 plan-runner 读取 PLAN.lisp 自动应用"
      :status "architecture-designed; runtime v2 已能消费 PLAN.lisp 节点 :dispatch-strategy / :target-project / :parallelism (mission_plan execute_mode=internal 路径), 但完整 PLAN.lisp 自动推断 strategy 仍 code-alignment pending"
      :dag-scheduler-cross-ref "intent-layer pillar :: section action-instruction-actor :: actor plan-dag-scheduler — runtime v2 已 code-aligned partial; 节点 schema :dispatch-strategy / :target-project / :parallelism 是本 matrix 输入; scheduler s6 按本 matrix 路由 substrate (anchor: intent-layer.plan-dag-runtime-v2)"
      :flow-cross-ref "flow pillar :: F-intent-alignment-plan-execution-loop :: s6 :: dag-scheduler — 完整 11-stage 协议正文 (runtime v2 是 dispatch + lifecycle + failure-policy 子集)"
      (rule lisp-architecture-task
        :pattern "PLAN.lisp 仅修改 .missiond/v2/*.lisp / .missiond/intent-*.lisp / .missiond/workflows/*.lisp"
        :strategy "resident-lisp-architect-session — 复用常驻 slot, 不另开"
        :rationale "架构改动需要 cumulative context; fresh slot 反复读 27+ lisp 文件浪费 token")
      (rule code-alignment-task
        :pattern "PLAN.lisp 含 Rust / SQL / JS / TS / Swift 文件修改"
        :strategy "fresh-code-alignment-session — spawn 新 slot, project-root cwd"
        :rationale "代码同构需要隔离 + 任务自包含; 任务 .md 已是 self-contained contract; 完成后必须 scoped commit")
      (rule cross-pillar-scan
        :pattern "PLAN.lisp 含 N≥2 个独立可并行的扫描/读取/验证步骤"
        :strategy "fresh session + agent-team-hint"
        :rationale "并行 sub-agent 加速 anchor 定位; 但写入仍由主 agent 单点")
      (rule mixed-task
        :pattern "PLAN.lisp 既改 Lisp 又改代码"
        :strategy "拆 plan: Lisp 改动给 resident-lisp-session, 代码改动给 fresh-code-alignment-session; 用 mission_execution coordination 串接; 各自按 claim scope scoped commit"
        :rationale "单 slot 同时持两种上下文会互相污染; 最好按文件类型划分 phase")
      (rule emergency-throwaway
        :pattern "无需观测 / 无需 evidence / 一次性查询 (例: 临时调试问答)"
        :strategy "可退到 prompt-mode (claude -p) — fallback only"
        :rationale "spawn 成本不值得; 但任何会写文件 / 改状态的任务必须走 spawn"))

    (execution-strategy-record
      :desc "把 plan-runner 选择的策略写进 mission_execution 协调面, 让 evidence-collector 与 capability-usage-monitor 能事后回放"
      :field "execution.dispatch_strategy ∈ {resident-lisp / fresh-code-alignment / agent-team / mixed / prompt-fallback / unknown} (companion log meta) — 未知值归一化为 unknown"
      :writer "mission_execution(action=open) 接收 dispatch_strategy/target_project/requested_cwd 写入 companion log meta; plan-runner v0 (mission_plan execute_mode=internal target=mission_execution) 自动转发; plan_dag runtime v2 在 wave-based 并发下沿用 evidence transition + companion log 串行化"
      :consumer "evidence-collector 落到 evidence sidecar plan_runner_dispatch / plan_dag_node_dispatch entry (typed EvidenceEntry); mission_execution(list/status) 暴露给 capability-usage-monitor 回放"
      :status "code-aligned partial — schema + 写 companion log meta + list/status graceful read; runtime v2 跨节点 evidence sidecar 保留字段; ExecutionEvent::Opened 扩展 dispatch metadata 仍 pending (anchor: intent-layer.plan-dag-runtime-v2.execution-event-decision)"
      :durability-cross-ref "scoped-commit-handoff policy — completion 必须最终带 commit_hash 或 commit_status=blocked, 否则 audit 标记 durability gap"
      :implementation-targets ["crates/missiond-mcp/src/tools/knowledge/agent_execution.rs (open schema)"
                               "crates/missiond-daemon/src/handlers/knowledge/agent_execution.rs (action_open meta render + list/status graceful)"
                               "crates/missiond-daemon/src/handlers/knowledge/plan.rs (execute_mode=internal forwarding to mission_execution)"
                               "crates/missiond-daemon/src/handlers/knowledge/plan_dag.rs (runtime v2 per-node evidence transition typed)"
                               "crates/missiond-daemon/src/handlers/knowledge/evidence_collector.rs (typed EvidenceEntry; EventRef::unavailable 占位)"
                               "crates/missiond-core/src/event/events/execution.rs (ExecutionEvent — 字段扩展 pending)"]
      :pending ["ExecutionEvent::Opened 扩展 dispatch_strategy/target_project/requested_cwd"
                "ExecutionEvent::PlanNodeStateChanged 新增 (per-node FSM 转移广播; runtime v2 暂用 evidence sidecar plan_dag_node_dispatch entry)"
                "timeline / 其他查询面暴露 dispatch_strategy"
                "plan-runner 自动从 PLAN.lisp DAG 推断 strategy (auto-selection v1 已支持保守 hint)"
                "完整 PLAN DAG scheduler 跑通后 per-node companion log 写入路径 — 复用 mission_execution 12-action manager"
                "EventRef::unavailable → live ExecutionEvent id"])
      :dag-scheduler-cross-ref "intent-layer pillar :: section action-instruction-actor :: actor plan-dag-scheduler — runtime v2 已 code-aligned partial; 完整 11-stage scheduler 必须经 mission_execution(action=claim) 申请 claim_id (D010 教训)")

  ;; ──────────────────────────────────────────────────────────
  ;; B · flow pillar :: F-workstation-dispatch-policy
  ;; (was: intent-flow.lisp lines 1446–1512)
  ;; section-id roots: flow.workstation-dispatch-policy
  ;; ──────────────────────────────────────────────────────────
  (flow F-workstation-dispatch-policy
    :desc "MissionD 调度 ClaudeCode 工位的策略选择 — unified-entry pipeline s6 execution-runner 的子策略 narrative; 不重复 slot lifecycle 全文, 仅描述策略选择 → 派工口径"
    :status "operational-practice + code-aligned partial — dispatch_strategy 贯穿 mission_plan(execute internal) → mission_execution(open) → companion log meta → evidence sidecar; PLAN DAG runtime v2 wave-based 并发下用 typed EvidenceEntry 串行化; auto-推断 strategy/target/target_project + ExecutionEvent dispatch metadata 仍 pending (anchor: flow.workstation-dispatch-policy / intent-layer.plan-dag-runtime-v2.execution-event-decision)"
    :role "把 worker pillar :: section claudecode-workstation-orchestration 的 5 条 policy 与 unified-entry pipeline 的 stage s2/s4/s6 连成一条 narrative; 是 cross-ref 中枢, 不是新执行路径"
    :triggers
      ["F-intent-alignment-plan-execution-loop :: s2 alignment-authoring (Lisp 改动)"
       "F-intent-alignment-plan-execution-loop :: s4 plan-authoring (PLAN 起草)"
       "F-intent-alignment-plan-execution-loop :: s6 execution-runner (PLAN 执行)"
       "human request to dispatch a ClaudeCode workstation explicitly"]
    :ingress
      (entry "PLAN.lisp 节点 :dispatch-strategy + :target_project + :parallelism + 任务 .md 路径 (.missiond/claudecode/<topic>.md)")
    :stages
      ((s1 classify-task
          :at "intent-layer pillar :: plan-runner (architecture-designed)"
          :reads ["PLAN.lisp 节点 :dispatch-strategy" "节点改动文件清单 (Lisp / Rust / SQL / JS / 混合)" "节点 :parallelism 与 :target_project"]
          :decision-table-ref "worker pillar :: section claudecode-workstation-orchestration :: dispatch-decision-matrix"
          :outputs "strategy ∈ {resident-lisp / fresh-code-alignment / agent-team / mixed / prompt-fallback}")
       (s2 select-substrate
          :at "worker pillar :: section claudecode-workstation-orchestration"
          :branches
            ((resident-lisp        :substrate "复用现有 ClaudeCode lisp-architect slot via mission_pty_send / mission_task_delegate")
             (fresh-code-alignment :substrate "mission_pty_spawn 新 slot 或 mission_compute_slot create dynamic slot, project-root cwd")
             (agent-team           :substrate "fresh slot + 在任务 .md 中加入'使用 agent-team 提高效率'文字; ClaudeCode 主会话内并发 sub-agent")
             (mixed                :substrate "拆 plan 分阶段: Lisp 阶段 → resident-lisp; 代码阶段 → fresh-code-alignment; 用 mission_execution 串接")
             (prompt-fallback      :substrate "claude -p 一次性 prompt; 仅 daemon 不可用或 throwaway 查询时使用")))
       (s3 enforce-cwd-and-context
          :at "worker pillar :: section pty :: invariant project-root-spawn-cwd"
          :enforcement "spawner::spawn_tracked_slot 强制 cwd = target_project_root; existing slot 复用前校验 slot.project_root == target_project_root"
          :code-alignment "code-aligned for spawn cwd; 自动 mismatch detection 已存在")
       (s4 record-strategy
          :at "tools pillar :: mission_execution(action=open) — dispatch_strategy/target_project/requested_cwd 已写入 companion log meta"
          :writes ["companion log meta :dispatch-strategy / :target-project / :requested-cwd" "evidence sidecar plan_runner_dispatch entry"]
          :consumer "evidence-collector 落 evidence sidecar; capability-usage-monitor 后续统计策略命中率"
          :status "code-aligned partial — companion log meta 持久化已实现, plan-runner internal mode 自动转发也已实现; ExecutionEvent 扩展 dispatch metadata 仍 code-alignment pending (companion-log durable only / event metadata future)"
          :implementation-targets ["crates/missiond-mcp/src/tools/knowledge/agent_execution.rs (open schema: dispatch_strategy/target_project/requested_cwd)"
                                   "crates/missiond-daemon/src/handlers/knowledge/agent_execution.rs (action_open meta render + list/status meta read + legacy log graceful read)"
                                   "crates/missiond-core/src/event/events/execution.rs (ExecutionEvent — dispatch metadata 仍未扩展)"]
          :pending ["ExecutionEvent::Opened 扩展 dispatch_strategy/target_project/requested_cwd 字段"
                    "list/status 读路径之外的查询面 (例如 timeline 查询) 暴露 dispatch_strategy"])
       (s5 dispatch-and-monitor
          :at "tools/worker pillars"
          :substrate ["mission_pty_spawn / mission_pty_send / mission_pty_read"
                      "mission_task_delegate (resident slot 上挂新任务)"
                      "mission_compute_slot (动态 slot)"
                      "mission_execution claim/heartbeat/release"]
          :monitor "mission_pty_screenshot / mission_pty_status; PTY extractor anomaly; ControlTree pause cascade"))
    :egress
      (writes ["execution row (with dispatch_strategy field — future)"
               "PTY session log (via spawn substrate)"
               "tool_calls (via gen_gateway audit)"
               "evidence sidecar (via mission_plan record_evidence)"]
       reads ["PLAN.lisp 节点 :dispatch-strategy" "worker pillar dispatch-decision-matrix" "ProjectRegistry"]
       downstream ["F-intent-alignment-plan-execution-loop :: s7 evidence-collection"
                   "F-execution-log-governance"
                   "F-scoped-commit-handoff"
                   "F-capability-usage-monitoring (后续统计)"]
       returns "strategy choice + spawned/reused slot id + execution_id (when manager opened)")
    :anti-patterns
      ["不要为单次 Lisp 改动新开 fresh ClaudeCode slot — 浪费上下文"
       "不要为代码同构复用常驻 Lisp slot — cwd 不对, 上下文污染"
       "不要让多个并行 sub-agent 同时写同一 Lisp 块 — 结构漂移"
       "不要默认走 claude -p — 无 evidence 闭环, 不可观测"
       "不要让并行写入任务只留下 completion report 而没有 scoped commit hash"]
    :no-new-tool "本 flow 不引入新 tool; 全程复用 mission_pty_* / mission_compute_slot / mission_task_delegate / mission_execution / mission_plan"
    :tools-backref ["mission_pty_spawn" "mission_pty_send" "mission_pty_read"
                    "mission_compute_slot" "mission_task_delegate"
                    "mission_execution" "mission_plan"])

  ;; ──────────────────────────────────────────────────────────
  ;; C · intent-layer pillar :: section unified-entry-pipeline ::
  ;;     workstation-dispatch-policy (4+ principle)
  ;; (was: intent-intent-layer.lisp lines 743–788)
  ;; section-id root: intent-layer.unified-entry-pipeline.workstation-dispatch-policy
  ;; ──────────────────────────────────────────────────────────
  (workstation-dispatch-policy
    :desc "把 ClaudeCode 工位/会话的选择规则挂在 unified-entry-pipeline 的元层语义层 — 决策语义在本 pillar, runtime mechanics 在 worker pillar, narrative 在 flow pillar"
    :status "operational-practice (人工已用) + architecture-designed (Lisp 已声明); plan-runner 自动选路 code-alignment pending"
    :worker-pillar-cross-ref "worker pillar :: section claudecode-workstation-orchestration"
    :flow-pillar-cross-ref "flow pillar :: F-workstation-dispatch-policy + F-intent-alignment-plan-execution-loop :: s2 / s4 / s6"
    :tools-pillar-cross-ref "mission_pty_spawn / mission_pty_send / mission_compute_slot / mission_task_delegate / mission_execution / mission_plan"

    (principle alignment-author-mode-selection
      :rule "alignment-author 默认 mode B (resident ClaudeCode lisp-architect slot); 只在短任务无上下文需求时退到 mode A (direct LLM)"
      :rationale "Lisp 架构改动需要 cumulative context — 已加载的 v2/*.lisp 文件结构是 asset, 重新定位成本高"
      :status "operational-practice; mode 选择仍由人工决策, plan-runner 自动选 mode 待 code-alignment")

    (principle plan-runner-dispatch-selection
      :rule "plan-runner 读 PLAN.lisp 节点 :dispatch-strategy + :target_project, 按 worker pillar dispatch-decision-matrix 选策略"
      :strategies
        [(resident-lisp        "PLAN 仅改 .missiond/**/*.lisp / workflows/*.lisp → 复用常驻 lisp-architect slot")
         (fresh-code-alignment "PLAN 改 Rust/SQL/JS/TS/Swift → spawn 新 slot, project-root cwd")
         (agent-team           "PLAN 含独立可并行扫描/读取/验证步骤 → 在任务 .md 中加 'agent-team 提高效率' 提示")
         (mixed                "PLAN 既改 Lisp 又改代码 → 拆 phase, 用 mission_execution 串接两类 slot")
         (prompt-fallback      "fallback only — daemon 不可用 / throwaway 查询")]
      :status "architecture-designed; plan-runner actor 待实现")

    (principle task-md-self-contained-contract
      :rule "fresh sessions 的任务由 .missiond/claudecode/<topic>.md 描述 — 必须 self-contained: scope / no-goals / acceptance / files / risks / 验收命令"
      :owner "alignment-author 与 plan-compiler 联合产出 task .md, 写入边界来自 alignment-review-gate / plan-review-gate"
      :status "operational-practice — 当前 .md 由人工/常驻 Lisp slot 协作产出; plan-compiler actor 自动生成 .md 待 code-alignment")

    (principle resident-context-not-substitute-for-checker
      :rule "常驻 Lisp 会话的上下文是 asset, 不能替代 checker 与 explicit task file"
      :enforcement
        ["每次 alignment / PLAN 写完必须运行 node scripts/check-architecture-lisp.mjs --all-v2"
         "任务 .md 路径必须显式声明; 不允许靠常驻会话记忆代替"
         "checker 失败必须先修, 不允许靠'下次会话再说'兜底"]
      :status "operational-practice; checker 已 code-aligned; 自动 enforce 仍依赖 plan-runner 的 acceptance 验证"
      :anti-pattern "不要因为常驻 slot 上下文好就跳过 checker 与任务文件 — 上下文漂移会让它认错")

    (principle spawn-over-prompt-mode
      :rule "默认通过 spawn / resident workstation 调度 ClaudeCode; 尽量少用 claude -p"
      :rationale "spawn 工位有 cwd / MCP config / session logs / execution evidence / 可监控状态; -p 难纳入 evidence/workflow/capability-usage 闭环"
      :enforcement "plan-runner 默认走 spawn 路径; 显式 dispatch_strategy=prompt-fallback 才允许"
      :status "architecture-designed; runtime enforce 待 code-alignment")

    (principle project-root-cwd-contract
      :rule "fresh code-alignment 工位 cwd 必须 = target_project_root; 常驻 Lisp 会话 cwd = missiond 仓库根 (本仓 .missiond/v2/*.lisp 是工作面)"
      :enforcement "spawner::spawn_tracked_slot 已强制 cwd 校验; reuse slot 必须 slot.project_root == target_project_root"
      :status "code-aligned for spawn cwd enforcement; 任务 .md 中 :target_project 字段 plan-runner 消费 pending"))

  ;; ──────────────────────────────────────────────────────────
  ;; D · trace-derived router policy (wave 23 architecture draft)
  ;; section-id root: worker.section.trace-derived-router-policy
  ;; ──────────────────────────────────────────────────────────
  (trace-derived-router-policy
    :desc "基于 session-trace 的后续 LLM router / backend selection 草案 — 用真实执行轨迹压缩 ClaudeCode 步数, 但本 wave 不替换 ClaudeCode"
    :status "architecture-designed — wave 23 只完成 trace collection / propagation / descriptive analyzer; runtime router replacement 未 code-aligned"
    :truth-sources ["session-trace.lisp (append-only factual telemetry)"
                    "task-report.lisp + shared-memory.lisp + scoped commit hash"
                    "mission_execution companion log / evidence sidecar"]
    :router-non-goal "单次 trace 不能直接决定 backend; analyzer only describes bottlenecks and anti-patterns"
    :backend-classes
      [(claudecode
         :role "default broad coding/workstation backend"
         :use-when "任务需要交互式 codebase exploration, file edits, tests, and scoped commit handoff"
         :avoid-when "trace shows repeated deterministic checker failures or pure mechanical edits")
       (missiond-llm-router
         :role "future policy engine selecting model/backend from contract shape + trace history"
         :status "future-code-alignment; no runtime in wave 23")
       (deterministic-checker
         :role "schema/checker/verifier-only lane"
         :use-when "task contract asks only to validate Lisp/report/memory/trace artifacts")
       (patch-worker
         :role "small bounded edit backend"
         :use-when "write-scope is tiny, acceptance is deterministic, and trace history says ClaudeCode setup dominates runtime")
       (verifier-worker
         :role "post-run proof and regression lane"
         :use-when "task is read-only verification after commit")]
    :policy-inputs [:contract-kind :write-scope-size :must-not-touch-risk :acceptance-command-class :historical-tool-count :historical-stall-seconds :trace-bottleneck-tags]
    :must-preserve ["Lisp task-contract remains dispatch SSOT"
                    "scoped commit / report / shared-memory / session-trace stay separate artifacts"
                    "No `claude -p` fallback for workstation write tasks"
                    "Router decisions require explainable policy record, not opaque prompt intuition"]))
