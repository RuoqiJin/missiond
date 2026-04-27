;; ═════════════════════════════════════════════════════════════
;; MissionD — Execution Governance Shard (L2 split, wave 15 task 02)
;; 目标: mission_execution 12-action manager + scoped commit handoff +
;;       dispatch metadata + plan node event governance
;; 来源: 由 wave 15 task 02 从 intent-flow.lisp + intent-tools.lisp 物理 split;
;;       内容与原主 Lisp byte-identical (no-content-mutation, R5)
;; 父引用: intent-flow.lisp + intent-tools.lisp + intent-intent-layer.lisp
;;         各留 stub 指回本 shard
;; 拆分计划: architecture-dsl.lisp :: l2-shard-split-plan ::
;;           shard intent-execution-governance
;; ═════════════════════════════════════════════════════════════

(execution-governance-shard
  :version "v1"
  :origin "wave 15 task 02 — physical split from flow + tools parents"
  :status "code-aligned — 12-action manager + ExecutionEvent emission + dispatch metadata 全 11 variants + scoped commit daemon enforce v0 (wave 16-06 commit 591d288) + scoped-commit worktree preflight v0 (wave 18-08 commit f171ae6) + execution task-contract completion verification v0 (wave 19-08 commit 405d13b) + execution preflight task-contract scope v1 (wave 20-03 commit fe835e8 加 8 contract-scope 字段, 0 mutating git) + task-scope-index-guard v1 (wave 20-01 commit 1fc0fd6) + ExecutionEvent legacy metadata sweep v0 (wave 20-09 commit 6e01e3f, 11 variants 全闭环). wave 21 task 01-03 (commits 44c74df/1335fa7/308426e) 加 hooks-path installer v1 (opt-in repo-local only) + task-run verifier v1 (三合一 + 14 forbidden git verb proof + dogfood self-verify) + execution report-verifier integration v1 (mission_execution(complete) 加 4 新字段 task_run_verifier_status / shared_memory_path / verifier_diagnostics / verified + verified=true 触发 daemon-internal enforce_verified_completion sexp cross-check 无新 dep + 4 structured error codes + daemon_never_invokes_mutating_git unit-pinned, daemon NEVER spawns Node — wave21-02 verifier 仍 out-of-process 权威). brief → preflight (含 contract scope) → staged guard → commit → verifier (post-commit) → execution complete verify (daemon-internal verified gate) 六段闭环. **wave 22 task 01 (commit 49555c4) 加 hooks default-on doctor v2** — install-missiond-hooks.mjs default mode (无 flag) 升级到 = --check 只读 doctor (read-only preflight 默认跑); --install 仍唯一 mutation entry; doctor JSON payload 加 :severity (ok|preflight-drift), :reason (4 codes: aligned|hooks-path-unset|hooks-path-wrong|hook-file-missing), :advice, :install_command; renderer renderHooksDoctorPreflight() 块在每 :commit :required brief commit section 上方提示 caller (双指令 check-missiond-hooks --json + install-missiond-hooks --install); dry-fixture 8→11 covers 4 doctor states + install-refuses-when-hook-file-missing + adviceFor() install-command surface assertions; **opt-in repo-local only — git config 真 mutation 仍 caller 显式 --install** (default-on doctor ≠ default-on real install). **wave 22 task 02 (commit 02ac627) 加 execution auto-run-verifier v2** — daemon-internal `auto_run_task_run_verifier` 8 cross-checks in-process (task_contract_loadable / task_report_loadable / task_report_schema / task_id_matches_contract / commit_hash_matches_report / shared_memory_loadable / shared_memory_schema / shared_memory_completion_for_task); 当 task_contract_path + task_report_path + shared_memory_path + commit_hash 4 路径全提供时 daemon 自跑 cross-check (read-only file inspection — 绝不 spawn Node/shell/mutating git); verification_source='daemon-auto-verifier' 主路径; legacy verified=true with missing paths 降级 verification_source='legacy-caller-claim' verifier_status=unknown verifier_diagnostics 列出 missing paths 不硬拒 (back-compat); 3 新错误码 SHARED_MEMORY_REQUIRED / SHARED_MEMORY_MALFORMED / SHARED_MEMORY_NO_COMPLETION_FOR_TASK; wave21-03 enforce_verified_completion helper 仍保留 (legacy verified=true 路径仍 honored degrade legacy-caller-claim); 8 new tests (auto_verifier_accepts_aligned_quartet / auto_verifier_rejects_missing_shared_memory / auto_verifier_rejects_shared_memory_schema_mismatch / auto_verifier_rejects_shared_memory_without_completion_for_task / shared_memory_projector_extracts_required_fields / completion_task_id_ignores_entry_without_task_slot / auto_verifier_reuses_wave21_03_codes_for_report_task_id_mismatch / auto_verifier_accepts_short_long_sha_prefix_overlap); daemon NEVER spawns Node — wave21-02 verifier 仍 out-of-process 权威, daemon-internal 用 in-tree sexp parser. brief → preflight → staged guard → commit → verifier (post-commit out-of-process) → execution complete verify (daemon-internal **auto_run_task_run_verifier**, 4 路径全提供时主路径 'daemon-auto-verifier') 六段闭环 wave-22 强化. 完整 enforce-by-default + git config core.hooksPath .githooks 默认 default-on real install (wave22-01 仍 doctor only) + report-contract checker 完全 auto-invoke from daemon (wave22-02 已大幅推进, 但 caller 必须仍提供 4 路径才触发 daemon-auto-verifier) 仍 future"
  :stability "section-ids preserved (R008 + R016); content byte-identical to pre-split parents"
  :wave-23-status-summary
    ["mission_execution(open/preflight_commit/complete) 可按 session_trace_path append session-trace.lisp; 失败只返回 trace_warning, 不破主 action"
     "trace append 由 Rust 直接文件 I/O 完成, 不 shell out, 不 spawn Node"
     "trace 用于后续执行治理分析; scoped commit / verifier / shared-memory 仍是完成证明主链"]

  ;; ──────────────────────────────────────────────────────────
  ;; A · flow pillar :: F-execution-log-governance
  ;; (was: intent-flow.lisp lines 1544–1586)
  ;; section-id root: flow.execution-log-governance
  ;; ──────────────────────────────────────────────────────────
    (flow F-execution-log-governance
      :desc "mission_execution manager → execution companion log → claim/decision/deviation/completion governance"
      :triggers ["mission_execution(action=...)" "multi-agent pillar/code-alignment work session"]
      :status "code-aligned partial; 12-action manager + Domain::Execution / ExecutionEvent emission implemented; scoped commit durability handoff architecture-designed"
      :protocol "memory pillar :: board helper agent-execution-coordination v0.5.2"
      :stages
        ((s1 open-or-load-execution
            :at "tools pillar mission_execution + worker pillar :: agent-execution-manager-interface"
            :action "open creates meta/id-counters/phase-tracker; list/status/audit loads existing companion logs")
         (s2 claim-scope
            :at "worker manager + memory protocol"
            :action "claim validates overlapping scope, allocates claim_id, sets lease_expires_at")
         (s3 keepalive-or-release
            :at "worker manager"
            :action "heartbeat extends lease; release closes claim with optional summary")
         (s4 record-operational-facts
            :at "worker manager"
            :action "deviate/decide/issue/complete allocate D/DC/I/COMP ids and append typed records")
         (s5 update-derived-indexes
            :at "execution companion log"
            :writes "active_claims / open_issues / unresolved_deviations / latest_decisions / completed_phases")
         (s6 audit-or-repair
            :at "worker manager + architecture-lisp checker"
            :checks ["paren balance" "ID monotonic/no duplicates" "claim overlap" "stale claims" "completed phase coverage" "open issue owners"]
            :repair "dry-run first; apply may only mark stale claims / rebuild derived indexes / suggest renumber")
         (s7 scoped-commit-handoff
            :at "worker pillar :: claudecode-workstation-orchestration + git"
            :action "after verification, stage only claimed files, commit scoped patch, then backfill commit_hash into completion/git-handoff"
            :flow-ref "F-scoped-commit-handoff")
         (s8 expose-status
            :at "tools pillar response + optional board progress"
            :returns "action receipt / status report / audit report"))
      :egress
        (:writes ["*-execution.lisp operational slots" "derived indexes" "git commit hash in completion/git-handoff"]
         :reads ["parent design Lisp" "execution companion log"]
         :emits ["ExecutionEvent::*" "optional BoardTaskStatusChanged when linked"]
         :returns "mission_execution JSON result")
      :tools-backref ["mission_execution"]
      :cross-pillar
        (memory "owns execution protocol + durable file shape")
        (worker "owns manager mechanics, leases, audit/repair runtime")
        (tools "owns future MCP schema")
        (intent-layer "keeps methodology separate from operational execution state"))

  ;; ──────────────────────────────────────────────────────────
  ;; B · flow pillar :: F-scoped-commit-handoff
  ;; (was: intent-flow.lisp lines 1588–1650)
  ;; section-id root: flow.scoped-commit-handoff
  ;; ──────────────────────────────────────────────────────────
    (flow F-scoped-commit-handoff
      :desc "执行共享内存层 → scoped git commit → commit_hash 回填, 解决并行工位报告完成但 live tree 成果被回退的问题"
      :status "architecture-designed + task-file operational-practice; daemon automated stage/commit/preflight pending"
      :why "execution companion Lisp 是 control plane, 记录 claim/lease/verification; git commit 是 durability plane, 固化实际文件修改. 两者必须互相引用."
      :triggers ["writing agent finished verification" "mission_execution(action=complete) with changed_files" "plan-runner node success"]
      :ingress
        (entry "execution_id + claim_id + claimed_scope + changed_files + verification result + commit policy")
      :stages
        ((s1 read-claim-scope
            :at "mission_execution(status)"
            :reads "claims.active_claims / completion candidate"
            :guard "claim must be active or just released by same claimer; stale/foreign claim requires issue/deviate")
         (s2 compute-changed-files
            :at "git worktree"
            :action "read git diff --name-only / git status --short; classify staged/unstaged/untracked"
            :guard "changed_files must be subset of claimed_scope or explicitly approved deviation")
         (s3 verify-before-stage
            :at "task acceptance"
            :requires ["task-specific tests" "git diff --check" "architecture-lisp checker if Lisp changed"]
            :failure "write mission_execution(issue) or completion commit_status=blocked; do not commit")
         (s4 stage-owned-files-only
            :at "git index"
            :action "git add exact file list from claimed_scope; never git add ."
            :preflight "git diff --cached --name-only must equal/subset claimed files")
         (s5 scoped-commit
            :at "git"
            :action "git commit with message carrying execution_id/phase/task"
            :result "commit_hash")
         (s6 backfill-completion
            :at "mission_execution(action=complete)"
            :writes "completion.changed_files / staged_files / verification / commit_hash / commit_status=committed")
         (s7 release-claim
            :at "mission_execution(action=release)"
            :action "release claim after commit hash is recorded; if commit blocked, release only with explicit summary"))
      :commit-policy
        ((code-alignment-new-session :commit "required")
         (parallel-code-agent        :commit "required")
         (resident-lisp-architect    :commit "wave-boundary-or-required")
         (exploration-only           :commit "not-required")
         (emergency-fix              :commit "required-after-verify"))
      :failure-modes
        ((scope-violation
           :symptom "staged file outside claimed_scope"
           :response "unstage offending file, write issue/deviation, request new claim")
         (verification-failed
           :symptom "tests/checker fail"
           :response "no commit; write issue or completion commit_status=blocked with blocker")
         (git-conflict-or-index-lock
           :symptom "parallel worker holding index / conflict markers"
           :response "stop and report; do not stash/reset/checkout other scopes")
         (commit-succeeded-backfill-failed
           :symptom "commit exists but execution log update failed"
           :response "emit issue + retry complete with commit_hash; commit remains durability truth"))
      :egress
        (writes ["git commit object" "execution companion log completion/git-handoff fields"]
         reads ["execution companion claims" "git diff/status" "task verification outputs"]
         returns "commit receipt or blocked handoff report")
      :tools-backref ["mission_execution"]
      :cross-pillar
        (memory "owns completion/git-handoff slot shape")
        (worker "owns workstation policy + git preflight mechanics")
        (flow "owns ordered handoff narrative")
        (intent-layer "plan-runner will call this after each code-writing node"))

  ;; ──────────────────────────────────────────────────────────
  ;; C · tools pillar :: implemented-surface mission_execution
  ;; (was: intent-tools.lisp lines 2772–2803)
  ;; section-id root: tools.surface.mission_execution
  ;; ──────────────────────────────────────────────────────────
    (implemented-surface mission_execution
      :status "code-aligned — 12-action manager + ExecutionEvent emission + dispatch_strategy companion log meta (open 接收并归一化 dispatch_strategy ∈ {resident-lisp/fresh-code-alignment/agent-team/mixed/prompt-fallback/unknown}; 写 :dispatch-strategy / 可选 :target-project / :requested-cwd 入 meta; list/status graceful read for legacy logs); ExecutionEvent::Opened 仍只带 {execution_id, parent_design, scope, owner, path}, dispatch metadata 入 ExecutionEvent 仍 pending"
      :actions ["open" "list" "claim" "heartbeat" "release" "deviate" "decide" "issue" "complete" "status" "audit" "repair"]
      :owner-boundary "tools owns schema; worker owns manager mechanics; memory owns execution protocol/file shape"
      :unified-pipeline-role "MissionD 统一入口 execution substrate — F-intent-alignment-plan-execution-loop :: s6 execution-runner 的底层协调面 (plan-runner v0 internal mode 已自动 forward dispatch_strategy)"
      :workstation-dispatch-record "execution coordination 已记录 PLAN 节点工位策略: dispatch_strategy 由 plan-runner(execute internal) 在调 open 时写入 companion log meta; evidence-collector / capability-usage-monitor 可事后回放; ExecutionEvent metadata 扩展仍 pending"
      :workstation-cross-ref "worker pillar :: section claudecode-workstation-orchestration :: execution-strategy-record + flow pillar :: F-workstation-dispatch-policy :: s4 record-strategy"
      :dispatch-strategy-field-status "code-aligned partial — companion log meta 持久化已落 (open schema 接受 + list/status 读取); ExecutionEvent::Opened 扩展 dispatch metadata 仍 pending (companion log durable only)"
      :scoped-commit-handoff "architecture-designed — mission_execution(action=complete) 未来接收 changed_files/staged_files/commit_hash/commit_status, 把 scoped git commit 作为 durability plane 回填 execution log; 当前可由任务书人工执行, schema/code enforce pending"
      :code ["crates/missiond-mcp/src/tools/knowledge/agent_execution.rs"
             "crates/missiond-daemon/src/handlers/knowledge/agent_execution.rs"]
      :implementation-targets ["crates/missiond-mcp/src/tools/knowledge/agent_execution.rs (schema: open 接受 dispatch_strategy/target_project/requested_cwd)"
                               "crates/missiond-daemon/src/handlers/knowledge/agent_execution.rs (open meta render + list/status meta read + legacy log graceful)"
                               "crates/missiond-core/src/event/events/execution.rs (ExecutionEvent — 当前未携带 dispatch metadata; 扩展 pending)"]
      (ingress
        :schema "action required; execution_id required except open/list; scope/phase/claimer fields per action; open 接受 dispatch_strategy/target_project/requested_cwd 写入 companion log meta; complete future fields changed_files/staged_files/commit_hash/commit_status"
        :callers ["multi-agent code/lisp alignment sessions" "intent-layer manager UI" "external MCP client" "unified entry pipeline plan-runner v0 internal mode"])
      (logic-core
        (step s1 "validate action + required fields; normalize execution_id/parent_design/scope; normalize dispatch_strategy (未知值 → unknown)")
        (step s2 "dispatch to worker :: agent-execution-manager-interface")
        (step s3 "manager reads/writes memory :: agent-execution-coordination slots; open 写 meta 含 dispatch_strategy 等")
        (step s4 "status/audit/repair return structured reports; list 读出 dispatch_strategy (legacy logs without meta 返空字符串/unknown)")
        (step s5 "future: complete validates scoped commit metadata (changed_files/staged_files subset of claim scope; commit_hash backfill)"))
      (egress
        :writes ["*-execution.lisp via manager (含 :dispatch-strategy / :target-project / :requested-cwd meta; future completion commit_hash fields)" "tool_calls audit"]
        :reads ["execution companion logs" "parent design Lisp"]
        :flow-ref "F-execution-log-governance / F-scoped-commit-handoff / F-intent-alignment-plan-execution-loop :: s6 execution-runner substrate / F-workstation-dispatch-policy :: s4 record-strategy"
        :event-bus "ExecutionEvent::* emitted for mutating/audit/repair actions; dispatch metadata 入 ExecutionEvent 仍 pending"
        :returns "mission_execution action JSON; list 行含 dispatch_strategy 字段")
      :pending ["ExecutionEvent::Opened 扩展 dispatch_strategy/target_project/requested_cwd 字段"
                "complete schema 扩展 changed_files/staged_files/commit_hash/commit_status + scoped commit audit"
                "timeline 等其他查询面暴露 dispatch_strategy"]))
