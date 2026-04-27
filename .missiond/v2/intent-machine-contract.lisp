;; ══════════════════════════════════════════════════════
;; MissionD v2 — Machine Contract Layer
;; 目的: 把 Lisp 从高密度说明书升级成 agent 之间的契约语言。
;;       task.lisp 是 SSOT; Markdown 是 ClaudeCode 执行视图。
;; ══════════════════════════════════════════════════════

(machine-contract-layer missiond
  :version "v0.6"
  :status "code-aligned full (wave 19 task 02-08 全 close + wave 20 task 01-09 + wave 21 task 01-08 propose+apply-gate + wave 22 task 01-07 explicit-gate-promotion + auto-verifier + smoke v4 闭环) — task-contract v1 schema + checker + verifier + report-contract v1 + shared-memory v1 + renderer dispatch brief v1 + plan task-contract emitter v0 + workstation task-contract consumer v0 + execution task-contract completion verification v0 (wave19-02..08). wave 20 进一步闭环: task-scope-index-guard v1 + renderer scoped-commit guard v2 + execution preflight task-contract scope v1 + machine-driven dispatch v0 (Lisp 真成 dispatch SSOT) + unified-entry machine-loop smoke v2 + review auto-answer policy v0 + ExecutionEvent legacy metadata sweep v0 (11 variants 全闭环). wave 21 task 01-08 全闭环 propose+apply-gate: hooks-path installer v1 (opt-in repo-local only) + task-run verifier v1 + execution report-verifier integration v1 + autonomous workstation LLM proposal v0 (propose only, applied=false / auto_spawn=false 永钉死) + plan inference apply gate v1 (persisted plan.sexp_text 永不 mutate / persist_inference_applied=false 永钉死) + LLM auto-approve proposal v0 (propose only, applied=false / requires_human=true 永钉死) + sonnet distill chain auto-apply v1 (双 opt-in required) + machine-contract autonomous loop smoke v3 (15 cross-wave invariants pinned). **wave 22 task 01-07 全闭环 explicit-gate-promotion + auto-verifier (commits 49555c4/02ac627/4b55cb4/fee6567/162a303/2423d4b/6b2125c)**: hooks default-on doctor v2 (install-missiond-hooks.mjs default mode = --check 只读 doctor / 唯一 mutation 仍是 --install / 4 reason codes aligned|hooks-path-unset|hooks-path-wrong|hook-file-missing / renderer renderHooksDoctorPreflight() 块在每 :commit :required brief commit section 上方 / 11 dry-fixtures 8→11 / 仍未 default-on real install — caller 必须显式 git config 才生效, wave22-01 commit 49555c4) + execution auto-run-verifier v2 (daemon-internal `auto_run_task_run_verifier` 8 cross-checks in-process / 当 task_contract_path + task_report_path + shared_memory_path + commit_hash 4 路径全提供时 daemon 自跑 cross-check / verification_source='daemon-auto-verifier' / 3 新错误码 SHARED_MEMORY_REQUIRED / SHARED_MEMORY_MALFORMED / SHARED_MEMORY_NO_COMPLETION_FOR_TASK / legacy verified=true with missing paths 降级 verification_source='legacy-caller-claim' 不硬拒 / 8 new tests / 绝不 spawn Node-shell-mutating-git, wave22-02 commit 02ac627) + review LLM approve apply gate v1 (apply_llm_auto_approve=true + proposal_hash + caller_approved=true 4 opt-in **6 道严格 gate**: G1 apply flag G2 deterministic SHA-256 hash matches G3 caller_approved=true G4 deterministic non-destructive G5 decision=approved G6 confidence=high / 3 新错误码 APPLY_GATE_MISSING_PROPOSAL_HASH / APPLY_GATE_PROPOSAL_HASH_MISMATCH / APPLY_GATE_INVALID_PARAM / hash mismatch fail-fast BEFORE DB mutation / 只 legacy quiet `action=approve` 路径触发 DB transition / wave21-06 5 invariants 全 preserved 5 dedicated tests / explicit review_decision 路径 gate 仍仅 informational, wave22-03 commit 4b55cb4) + persisted plan inference apply v2 (persist_inference=true + caller_approved=true + proposal_hash 4 opt-in apply gate / compute_inference_proposal_hash 32-hex SHA-256 / `plan_insert(version=max+1)` 加 (plan-inference-applied :inference-version v2 ...) annotation + `plan_supersede(old_id)` 真改 plan.sexp_text rollback 通过 predecessor 重 supersede / 2 新错误码 PERSIST_APPLY_MISSING_PROPOSAL_HASH / PERSIST_APPLY_PROPOSAL_HASH_MISMATCH / wave21-05 6 invariants 全 preserved 7 dedicated tests / I6 v1 `apply_gate.persist_inference_applied=false` 仍硬钉死, v2 用 SEPARATE `persisted_apply` block surface 状态, wave22-04 commit fee6567) + autonomous workstation true spawn v1 (auto_spawn=true + workstation_caller_approved + preflight_acceptable 4 opt-in apply gate / **12-rule gate matrix**: G1 auto_spawn opt-in / G2 bundle Suggested / G3 hash matches / G4 all proposals safety_status=safe / G5 all proposals confidence=high / G6 caller_approved=true / G7 preflight_acceptable=true / G8 task_contract_path supplied / G9 contract loads ok / G10 :write-scope non-empty / G11 :write-scope non-overlap with :must-not-touch / G12 proposed target=mission_task_delegate / 走 `mission_task_delegate` substrate **绝不 `claude -p`** / 3 新错误码 AUTO_SPAWN_INVALID_PARAM / AUTO_SPAWN_MISSING_PROPOSAL_HASH / AUTO_SPAWN_PROPOSAL_HASH_MISMATCH / 15 status taxonomy / wave21-04 4 invariants 全 preserved 4 dedicated tests, wave22-05 commit 162a303) + distill chain policy auto-sonnet v2 (auto_sonnet_policy ∈ {off, safe_after_rules, dry_run} 单一 policy 选择即 attestation / **dual opt-in 移除** — policy 选择即 explicit operator attestation / legacy auto_sonnet=true + auto_sonnet_approved=true 双 opt-in 仍 back-compat coexists / safe_after_rules 触发要 ALL 6 wave-20 rule pass + trigger=auto_safe + distill_mode != sonnet / dry_run 完整 evaluate 仅 surface 不 spawn / wave21-07 7 invariants 全 preserved 7 dedicated tests (I7 验证 4 块 coexistence), wave22-06 commit 2423d4b) + autonomous loop apply smoke v4 (9 new deterministic smoke tests 覆盖 wave22-02/03/04/05/06 / **22 cross-wave invariants pinned** wave21-04 4 + wave21-05 6 + wave21-06 5 + wave21-07 7 / no real LLM (synthesised proposal/bundle structs) / no real spawn (gate evaluators 终止于 Spawned 不调 substrate) / no mutating git (tempfile fixtures), wave22-07 commit 6b2125c). **7 wave-22 commits 全闭环了 explicit-gate-promotion 范式** — wave-21 propose-only 通道 (review LLM approve / persisted plan inference / autonomous workstation spawn / sonnet distill chain) 全部升级到 explicit-apply-gate; review framework 升 4 knob orthogonal (automation policy + auto-answer + auto-approve proposal + apply gate v1); plan inference 升 propose + v1 apply gate + **v2 persisted apply (plan_supersede rollback handle)**; workstation dispatch 升 propose + true spawn v1 (12-rule matrix); distill chain 升 policy-driven (deterministic 触发, 无 dual opt-in); execution verifier 升 daemon-internal auto-verifier 8 cross-checks; hooks 升 default-on doctor (read-only doctor 默认跑, mutation 仍 opt-in). 完整 LLM 自主全闭环 / Sonnet 真无任何 opt-in / git hooks default-on real install / frontend Lisp 仍 future"
  :schema ".missiond/tasks/schema/task-contract-v1.lisp"
  :checker "scripts/check-task-contract.mjs"
  :verifier "scripts/verify-task-contract.mjs"
  :report-schema ".missiond/tasks/schema/report-contract-v1.lisp"
  :report-checker "scripts/check-task-report.mjs"
  :shared-memory-schema ".missiond/tasks/schema/shared-memory-v1.lisp"
  :shared-memory-checker "scripts/check-task-memory.mjs"
  :session-trace-schema ".missiond/tasks/schema/session-trace-v1.lisp"
  :session-trace-checker "scripts/check-session-trace.mjs"
  :session-trace-analyzer "scripts/analyze-session-trace.mjs"
  :router-policy-schema ".missiond/tasks/schema/router-policy-v1.lisp"
  :router-policy-checker "scripts/check-router-policy.mjs"
  :router-recommendation-cli "scripts/recommend-task-backend.mjs"
  :trace-corpus-indexer "scripts/build-session-trace-index.mjs"
  :renderer "scripts/render-claudecode-task.mjs"
  :wave-23-status-summary
    ["session-trace v1 schema/checker/analyzer code-aligned (wave 23 task 01/06); trace writable remains explicit opt-in via :session-trace-writable, default false"
     "renderer/report contract surface trace paths and five explanation fields (wave 23 task 02); Markdown remains view, Lisp trace remains machine artifact"
     "mission_execution + plan/workstation paths can append/propagate trace in Rust (wave 23 task 04/05); daemon does not shell out for trace"
     "trace-derived router policy is architecture-designed only; trace analyzer describes bottlenecks, it does not choose models or replace ClaudeCode yet"]
  :wave-24-status-summary
    ["router-policy v1 schema/checker/seed code-aligned (wave 24 task 01); every policy must be dry-run-only and runtime_replacement=false"
     "trace corpus indexer code-aligned (wave 24 task 02); aggregates session traces into sorted JSON keys, no routing decision"
     "router recommendation CLI code-aligned (wave 24 task 03); read-only deterministic advisory, dry_run_only=true, no LLM/spawn/git"
     "mission_plan router_policy_mode=dry_run surface code-aligned partial (wave 24 task 04); applied=false literal, apply/auto/unknown rejected before plan lookup"
     "renderer router policy context code-aligned (wave 24 task 05); advisory/dry-run only text, no backend switching instruction"
     "full-chain smoke code-aligned (wave 24 task 06); Node/Rust/renderer invariants pinned; runtime backend replacement remains pending"]
  :wave-25-status-summary
    ["router-policy corpus evaluator code-aligned (wave 25 task 01 commit 8dbe85f); real corpus 67 tasks, backend distribution claudecode=49 / deterministic-checker=14 / verifier-worker=4, confidence high=6 / medium=5 / low=56, schema missiond.router-policy-evaluation.v0"
     "report-contract router recommendation fields code-aligned (wave 25 task 02 commit 7709031); seven optional flat fields plus strict atom-only booleans for router_applied/router_dry_run_only"
     "mission_plan router trace-index confidence code-aligned-partial (wave 25 task 03 commit bd2b5a3); optional router_policy_trace_index_path, statuses used/missing/unreadable/malformed, absent path omitted, mode=off blocks I/O"
     "renderer router recommendation command code-aligned (wave 25 task 04 commit e1fdbe4); brief renders parameterized recommend-task-backend command and MAY report-field guidance, still never shells out"
     "router measurement smoke code-aligned (wave 25 task 05 commit 0f5d857); CLI/Rust parity when trace evidence >=5, dry_run_only/applied/runtime_replacement invariants pinned, no runtime backend replacement"]

  (purpose
    "S-expressions carry machine boundaries: ownership, dependencies, acceptance, commit policy, review gate, rollback, evidence."
    "Markdown remains a rendered view for current ClaudeCode ergonomics."
    "MissionD plan-runner can later dispatch directly from Lisp without parsing natural-language task briefs.")

  (artifact-roles
    (intent-alignment-lisp
      :role "why / boundary / non-goal / acceptance intent"
      :machine-contract "records objective, scope, affected pillars, explicit non-goals, review gate owner")
    (plan-lisp
      :role "how / executable DAG / node dispatch / acceptance / rollback"
      :machine-contract "records node ids, dependencies, target tool, dispatch strategy, project root, claim/lease, review gate, commit policy")
    (workflow-lisp
      :role "reuse / distillation / trigger and match rules"
      :machine-contract "records applicability, parameters, disabled cases, evidence requirements, version chain")
    (shared-memory-lisp
      :role "runtime ledger"
      :machine-contract "records claims, decisions, issues, evidence, commit handoff, resume pointers")
    (session-trace-lisp
      :role "factual telemetry"
      :machine-contract "records dispatch / observation / completion events with elapsed time, token/tool counts, blockers, retries, and artifacts; append-only; never used as model-routing authority by itself")
    (router-policy-lisp
      :role "advisory backend policy"
      :machine-contract "records explainable backend recommendations derived from task contract shape and trace corpus; must declare :dry-run-only true and :runtime-replacement false")
    (task-lisp
      :role "dispatch contract"
      :machine-contract "records write-scope, must-not-touch, dependencies, acceptance commands, commit scope-check, report fields"))

  (pipeline
    (s1-author-task-contract
      :input "operator / MissionD plan-runner objective"
      :output ".missiond/tasks/<wave>/<task-id>.lisp"
      :guard "task-contract checker must pass")
    (s2-render-claudecode-view
      :input "task.lisp"
      :output ".missiond/claudecode/<task-id>.md"
      :command "node scripts/render-claudecode-task.mjs <task.lisp>"
      :note "renderer refuses overwrite unless --force")
    (s3-dispatch
      :input "rendered Markdown + machine contract id"
      :substrate "resident-lisp / fresh-code-alignment / agent-team / workstation-dispatch"
      :rule "Markdown is compatibility view; task.lisp remains SSOT")
    (s4-report
      :input "ClaudeCode report + scoped commit"
      :output "shared-memory.lisp / mission_execution companion log / evidence sidecar")
    (s5-verify
      :input "task.lisp + report + git diff/commit"
      :checks ["write-scope subset" "acceptance commands" "commit message" "must-not-touch unchanged" "task-id in commit message" "commit hash exists" "report status / acceptance_results / commit_hash present"]
      :command "node scripts/verify-task-contract.mjs <task.lisp> --commit <hash> [--json] [--dry-fixture]"
      :report-checker "node scripts/check-task-report.mjs <report.lisp> [--dry-fixture]"
      :shared-memory-checker "node scripts/check-task-memory.mjs <shared-memory.lisp> [--dry-fixture]"
      :status "code-aligned (wave 19 task 02 commit 77f1f2b + task 03 commit ba58f20 + task 04 吸入 commit 77f1f2b)"
      :future "auto-invoke verifier inside mission_execution(complete) when task_contract_path supplied (wave 19 task 08 已加 metadata 钩子, daemon 仍由 caller 触发 verifier)")
    (s6-trace
      :input "task.lisp + execution/report metadata + optional runtime observation"
      :output ".missiond/tasks/<wave>/session-trace.lisp"
      :command "node scripts/check-session-trace.mjs <session-trace.lisp> && node scripts/analyze-session-trace.mjs <session-trace.lisp>"
      :status "code-aligned partial (wave 23 task 01/04/05/06)"
      :rule "trace is telemetry and analysis input; router policy consumes aggregated trace later, never a single trace event directly")
    (s7-router-dry-run
      :input "task.lisp + router-policy-v1.lisp + optional trace corpus index"
      :output "router recommendation block / mission_plan router_recommendation response"
      :command "node scripts/build-session-trace-index.mjs <trace-root> && node scripts/recommend-task-backend.mjs --task <task.lisp> --policy .missiond/router/router-policy-v1.lisp --trace-index <index.json>"
      :status "code-aligned partial (wave 24 task 01-06 + wave 25 task 01-05 measurement loop)"
      :rule "advisory only: dry_run_only=true, applied=false, runtime_replacement=false; plan.rs dry-run surface never switches backend"
      :measurement "wave 25 adds corpus evaluator + report fields + trace-index confidence scoring + CLI/Rust parity smoke; measurement may raise confidence, never applies routing"))

  (task-report-v1
    :required-fields [:task_id :status :commit_hash :files_changed :acceptance_results :scope_deviations :notes]
    :status-values [draft in_progress blocked done]
    :reject-conditions ["missing task_id" "invalid status enum" "empty acceptance_results when status=done" "absolute file paths"]
    :checker "scripts/check-task-report.mjs (10 fixtures, --dry-fixture)"
    :sample ".missiond/tasks/wave19/reports/wave19-00-machine-contract-pilot.report.lisp")

  (shared-memory-v1
    :purpose "design-time 共享笔记 (区分于 mission_execution claims slot — claims 是 runtime 强协议, ledger 是 design-time 软协议)"
    :entry-types [claim observation blocker completion correction handoff]
    :required-per-entry [:entry-id :task-id :timestamp-or-seq :touched-files-repo-relative]
    :reject-conditions ["duplicate entry-id" "invalid timestamp" "absolute file paths" "empty entry"]
    :write-rule "agents append entries only inside their claimed write-scope; ledger 本身是 sole shared write target for coordination"
    :checker "scripts/check-task-memory.mjs (13 fixtures, --dry-fixture)"
    :seed ".missiond/tasks/wave19/shared-memory.lisp")

  (session-trace-v1
    :purpose "把长跑 ClaudeCode / workstation / verifier 会话的真实步骤变成 append-only S-expression telemetry, 用于后续步数优化与 router policy 设计"
    :schema ".missiond/tasks/schema/session-trace-v1.lisp"
    :checker "scripts/check-session-trace.mjs"
    :analyzer "scripts/analyze-session-trace.mjs"
    :status "code-aligned partial — wave 23 task 01 schema/checker + task 04 daemon append + task 05 plan/workstation propagate + task 06 descriptive analyzer"
    :writable-opt-in ":session-trace-writable true; default false"
    :event-types [dispatch observation completion failure]
    :minimum-facts [:event-id :task-id :phase :timestamp-or-seq :actor :elapsed-ms]
    :analysis-facts [:tool-call-count :token-estimate :stall-seconds :retry-count :write-method :blocked-reason]
    :non-goals ["do not infer model replacement from one run"
                "do not mutate task/report/shared-memory"
                "do not store chain-of-thought"
                "do not make router decisions in analyzer"])

  (trace-derived-router-policy
    :purpose "把 session traces 聚合成后续 LLM router / worker backend selection 的策略输入"
    :status "code-aligned partial — wave 24 adds router-policy v1 schema/checker/seed + corpus index + recommendation CLI + mission_plan dry-run surface; no runtime router replacement"
    :truth-source "many verified session traces + reports + scoped commits; single trace is anecdote"
    :decision-boundary "analyzer produces bottleneck descriptors; router policy chooses backend only after explicit policy wave"
    :backend-classes [claudecode missiond-llm-router deterministic-checker patch-worker verifier-worker]
    :rule "backend selection must be explainable by contract shape, required tools, write-scope, risk, historical trace cost, and acceptance/verifier needs; never by natural-language vibes alone"
    :future-code-targets ["crates/missiond-daemon/src/handlers/knowledge/workstation_dispatch.rs"
                          "crates/missiond-daemon/src/handlers/knowledge/plan.rs"
                          "crates/missiond-daemon/src/handlers/knowledge/unified_entry.rs"
                          "scripts/analyze-session-trace.mjs"])

  (router-policy-v1
    :purpose "machine-readable advisory policy for backend recommendation"
    :schema ".missiond/tasks/schema/router-policy-v1.lisp"
    :seed ".missiond/router/router-policy-v1.lisp"
    :checker "scripts/check-router-policy.mjs"
    :status "code-aligned — wave 24 task 01"
    :required-safety [:dry-run-only :runtime-replacement]
    :invariants ["every valid policy must set :dry-run-only true"
                 "every valid policy must set :runtime-replacement false"
                 "runtime-replacement true is a checker error"
                 "policy seed has three advisory rules: docs→claudecode, code-alignment+checker→deterministic-checker, review/smoke→verifier-worker"])

  (trace-corpus-index-v0
    :purpose "aggregate many session-trace ledgers into a deterministic corpus summary"
    :indexer "scripts/build-session-trace-index.mjs"
    :status "code-aligned — wave 24 task 02"
    :top-level-keys [bottleneck_tags by_backend by_task by_wave schema source_files thresholds totals]
    :thresholds "reuses wave 23 analyzer thresholds: long-running >= 1800000ms, high-retry >= 3, many-failures >= 2, no-completion dispatch>=1 complete=0"
    :non-goal "does not recommend backend and does not mutate traces")

  (router-recommendation-v0
    :purpose "read-only deterministic backend recommendation for a single task contract"
    :cli "scripts/recommend-task-backend.mjs"
    :status "code-aligned — wave 24 task 03"
    :output-schema "missiond.router-recommendation.v0"
    :guarantees ["dry_run_only is always true"
                 "no mutation, no shell, no LLM, no HTTP, no git"
                 "no matched rule falls back to claudecode confidence=low reason=insufficient_trace_history"
                 "policies with runtime_replacement=true or dry_run_only!=true are rejected"])

  (mission-plan-router-dry-run-surface-v0
    :purpose "Expose router recommendation through mission_plan execute as informational dry-run surface"
    :handler "crates/missiond-daemon/src/handlers/knowledge/plan.rs"
    :mcp-schema "crates/missiond-mcp/src/tools/knowledge/plan.rs"
    :status "code-aligned partial — wave 24 task 04"
    :args [:router_policy_mode :router_policy_path]
    :mode-contract ["absent/off => legacy response shape with no recommendation block"
                    "dry_run => response contains router_recommendation with applied=false literal"
                    "apply/auto/unknown => INVALID_PARAM before plan lookup"]
    :boundary "daemon implementation is pure Rust and independent from Node CLI; it does not spawn scripts and does not load trace index")

  (router-policy-corpus-evaluator-v0
    :purpose "Evaluate router-policy recommendations over the accumulated task-contract corpus, so router work is measurable instead of anecdotal"
    :cli "scripts/evaluate-router-policy-corpus.mjs"
    :status "code-aligned — wave 25 task 01"
    :schema "missiond.router-policy-evaluation.v0"
    :top-level-keys [by_backend by_confidence fallback_count per_task policy_path rejected_count schema tasks_root totals trace_index_source]
    :real-corpus "67 task contracts evaluated; claudecode=49, deterministic-checker=14, verifier-worker=4; confidence high=6, medium=5, low=56; fallback_count=43; rejected_count=0"
    :guarantees ["read-only"
                 "no shell, no git, no LLM, no HTTP"
                 "builds trace index in-process when --trace-index is absent"
                 "skips schema/reports/shared-memory/session-trace/parallel-dispatch-index artifacts"])

  (router-recommendation-report-fields-v0
    :purpose "Allow ClaudeCode reports to echo router dry-run recommendation evidence without making it load-bearing"
    :schema ".missiond/tasks/schema/report-contract-v1.lisp"
    :checker "scripts/check-task-report.mjs"
    :status "code-aligned — wave 25 task 02"
    :optional-fields [:recommended_backend :router_confidence :router_policy_path :router_dry_run_only :router_applied :router_reasons :router_trace_index_path]
    :strictness ["backend enum: claudecode / missiond-llm-router / deterministic-checker / patch-worker / verifier-worker"
                 "confidence enum: high / medium / low"
                 "paths repo-relative only; no absolute path, no leading ~, no .."
                 "router_dry_run_only and router_applied must be literal atoms true/false, not strings"
                 "router_reasons must be a vector of non-empty strings"]
    :boundary "fields are completion evidence only; they do not select or apply backend routing")

  (mission-plan-router-trace-index-confidence-v1
    :purpose "Score dry-run router recommendation confidence from aggregated trace evidence in mission_plan execute"
    :handler "crates/missiond-daemon/src/handlers/knowledge/plan.rs"
    :mcp-schema "crates/missiond-mcp/src/tools/knowledge/plan.rs"
    :status "code-aligned partial — wave 25 task 03"
    :arg :router_policy_trace_index_path
    :trace-index-statuses [used missing unreadable malformed]
    :confidence-rule "when recommendation matched and max(by_task[board_task_id].events, by_backend[recommended_backend].events) >= 5 => high; 1..4 => medium; 0 => low; no-match => low"
    :compat ["path absent => trace_index_* fields omitted and wave24 byte-shape preserved"
             "mode=off/default => early return; even supplied trace-index path performs no I/O"
             "missing/unreadable/malformed trace-index is non-fatal and emits warning + fallback confidence"
             "daemon keeps independent Rust parser; no Node CLI spawn"])

  (renderer-router-recommendation-command-v1
    :purpose "Render exact advisory commands for humans while keeping Lisp task contract as SSOT"
    :renderer "scripts/render-claudecode-task.mjs"
    :status "code-aligned — wave 25 task 04"
    :command-template "node scripts/recommend-task-backend.mjs --task <relSource> --policy <routerPolicyPath> --json"
    :report-guidance "Report Contract section lists seven router fields as MAY fields"
    :boundary "renderer emits text only; it never shells out and never instructs backend switching")

  (router-policy-measurement-smoke-v1
    :purpose "Pin the measurable dry-run loop across Node CLI, Rust mission_plan surface, renderer/report contract, and static forbidden-pattern audit"
    :status "code-aligned — wave 25 task 05"
    :invariants ["policy runtime_replacement=false"
                 "dry_run_only=true"
                 "applied=false JSON bool literal"
                 "renderer remains advisory/dry-run only"
                 "report checker rejects router_applied=true and router_dry_run_only=false"
                 "mission_plan off/default byte-shape unchanged"
                 "CLI/Rust parity for rich trace evidence: claudecode/high at (5,5)"
                 "no shell-out, LLM, git, network, or runtime backend replacement"])

  (task-contract-v1
    :required-fields [:schema :title :kind :status :owner :goal :write-scope :must-not-touch :acceptance :commit]
    :schema-value "missiond.task-contract.v1"
    :status-values [draft ready running blocked done archived]
    :commit-scope-check-values [write-scope-only none not-required]
    :current-checker "validates required fields, non-empty write-scope/acceptance, explicit must-not-touch, commit message/scope-check, repo-relative paths, exact overlap between write-scope and must-not-touch")

  (current-files
    (schema-task ".missiond/tasks/schema/task-contract-v1.lisp")
    (schema-report ".missiond/tasks/schema/report-contract-v1.lisp")
    (schema-shared-memory ".missiond/tasks/schema/shared-memory-v1.lisp")
    (pilot ".missiond/tasks/wave19/wave19-00-machine-contract-pilot.lisp")
    (rendered-pilot ".missiond/claudecode/wave19-00-machine-contract-pilot.md")
    (sample-report ".missiond/tasks/wave19/reports/wave19-00-machine-contract-pilot.report.lisp")
    (seed-shared-memory ".missiond/tasks/wave19/shared-memory.lisp")
    (checker-task "scripts/check-task-contract.mjs")
    (verifier-task "scripts/verify-task-contract.mjs")
    (checker-report "scripts/check-task-report.mjs")
    (checker-shared-memory "scripts/check-task-memory.mjs")
    (schema-session-trace ".missiond/tasks/schema/session-trace-v1.lisp")
    (checker-session-trace "scripts/check-session-trace.mjs")
    (analyzer-session-trace "scripts/analyze-session-trace.mjs")
    (schema-router-policy ".missiond/tasks/schema/router-policy-v1.lisp")
    (seed-router-policy ".missiond/router/router-policy-v1.lisp")
    (checker-router-policy "scripts/check-router-policy.mjs")
    (indexer-session-trace-corpus "scripts/build-session-trace-index.mjs")
    (cli-router-recommendation "scripts/recommend-task-backend.mjs")
    (cli-router-policy-evaluator "scripts/evaluate-router-policy-corpus.mjs")
    (renderer "scripts/render-claudecode-task.mjs")
    (parser "scripts/lib/missiond_lisp.mjs")
    ;; wave 20 additions
    (scope-guard "scripts/task-scope-guard.mjs")
    (pre-commit-hook ".githooks/pre-commit")
    ;; wave 21 additions
    (hooks-installer "scripts/install-missiond-hooks.mjs")
    (hooks-doctor "scripts/check-missiond-hooks.mjs")
    (run-verifier "scripts/verify-task-run.mjs")
    ;; wave 22 additions (no new file — wave 22 升级在既有 file/handler 上加 mode/field/gate)
    (hooks-installer-default-mode-doctor "scripts/install-missiond-hooks.mjs (default mode = --check 只读 doctor; --install 仍唯一 mutation)"))

  (non-goals-v0
    ["Markdown remains the ergonomic ClaudeCode execution view, but wave 20-04 machine mode 已让 Markdown 不再 load-bearing — caller 可关 render_markdown=false 让 dispatch 直接读 Lisp contract; wave 21-08 + wave 22-07 smoke 三度钉死 task_brief_preview NEVER 进 artifact_refs."
     "Do not auto-dispatch from task.lisp until verifier/report loop exists (wave 19 closed + wave 20-04 machine-driven dispatch v0 + wave 21-02/03 task-run verifier + execution-side verified gate 已落 + **wave 22-02 (commit 02ac627) 加 daemon-internal auto_run_task_run_verifier 8 cross-checks in-process 当 task_contract_path + task_report_path + shared_memory_path + commit_hash 4 路径全提供时 daemon 自跑 (verification_source='daemon-auto-verifier'), legacy verified=true 降级 'legacy-caller-claim'**; remaining: 完全无 hint 的 autonomous spawn / report-contract checker auto-invoke (wave22-02 已大幅推进, 但 caller 必须仍提供 4 路径才触发 daemon-internal verifier)."
     "Do not auto-apply LLM/inference proposals without explicit gate — wave 22-03 (commit 4b55cb4) 加 review LLM approve apply gate v1 (apply_llm_auto_approve + proposal_hash + caller_approved 4 opt-in + 6 道 gate); wave 22-04 (commit fee6567) 加 persisted plan inference apply v2 (persist_inference + caller_approved + proposal_hash 4 opt-in + plan_insert(version=max+1) + plan_supersede(old) 真改 plan.sexp_text); wave 22-05 (commit 162a303) 加 autonomous workstation true spawn v1 (auto_spawn + workstation_caller_approved + preflight_acceptable 4 opt-in + 12-rule gate matrix + mission_task_delegate substrate **绝不 claude -p**); wave 22-06 (commit 2423d4b) 加 distill chain policy auto-sonnet v2 (auto_sonnet_policy=safe_after_rules **policy 选择即 attestation**, dual opt-in 移除); 4 路 LLM/inference 通道全部进入 'caller explicit opt-in + 6/12-rule gate + deterministic SHA-256 hash + structured fail-fast errors' 模式. 仍未实现: 完全 LLM 自主无任何 caller opt-in / Sonnet 真无任何 attestation."
     "Do not enable hooks default-on real install — wave 21-01 hooks installer 是 opt-in repo-local only; **wave 22-01 (commit 49555c4) 加 default-on doctor v2** (install-missiond-hooks.mjs default mode = --check 只读 doctor, --install 仍唯一 mutation; renderer renderHooksDoctorPreflight() 块在 commit section 上方提示 caller 跑 --install); 但 git config core.hooksPath .githooks 默认仍未启用 — caller 必须显式跑 --install 才生效; enforce-by-default real install 仍 future."
     "Do not use router recommendation as runtime replacement — wave 24 router-policy / indexer / CLI / mission_plan surface and wave 25 evaluator/report/trace-index confidence loop are advisory dry-run only; runtime_replacement=false, dry_run_only=true, and applied=false are hard boundaries."
     "Do not start frontend Lisp in this wave (continue postpone)."
     "Do not interpret arbitrary Common Lisp; this is MissionD data Lisp only."])

  (next-steps
    [";; wave 19 closures (all done):"
     "DONE wave19-02 — Add task-contract verifier: task.lisp + git commit -> pass/fail (5 项检查, --commit/--json/--dry-fixture, read-only 0 mutating git)."
     "DONE wave19-03 — Add report-contract Lisp shape for ClaudeCode completion reports (7 字段 + 10 fixtures + sample report)."
     "DONE wave19-04 — Add shared-memory ledger v0 (6 entry types + 13 fixtures + seed wave19/shared-memory.lisp). Note: 文件被 wave19-02 一并入 commit 77f1f2b, 功能正确但 commit 归属错."
     "DONE wave19-05 — Renderer dispatch brief v1 (4 新节 + agent-team literal 单实例 + verify command)."
     "DONE wave19-06 — mission_plan emit task.lisp for eligible workstation nodes (.missiond/tasks/generated/<plan_id>/<node_id>.lisp; emit before dispatch; default off byte-compat)."
     "DONE wave19-07 — workstation_dispatch consume task.lisp (overlay_contract + MalformedTaskContract SafeDescriptor; legacy brief byte-identical absent contract; 绝不 fall back claude -p)."
     "DONE wave19-08 — mission_execution(complete) record verifier_status / task_contract_path / task_report_path; enforce_scoped_commit + task_contract_path → require commit_hash + claim scope ⊇ contract write-scope; daemon 仍 read-only."
     ";; wave 20 closures (all done):"
     "DONE wave20-01 — task-scope-index-guard v1: scripts/task-scope-guard.mjs staged/commit 双 mode + .githooks/pre-commit 仅 MISSIOND_TASK_CONTRACT env 触发 + 9+3 fixtures + 0 mutating git (commit 1fc0fd6). Caveat: git config core.hooksPath .githooks 默认未启用."
     "DONE wave20-02 — renderer scoped-commit guard v2: render-claudecode-task.mjs Commit 节加 task-scope-guard --mode staged 子步 + MISSIOND_TASK_CONTRACT env prefix (commit b36cf6c)."
     "DONE wave20-03 — execution preflight task-contract scope v1: mission_execution preflight_commit 加 8 新字段对账 contract scope; 0 mutating git; legacy byte-compat (commit fe835e8)."
     "DONE wave20-04 — machine-driven dispatch v0: DispatchContractMode {Rendered, Machine} + dispatch_contract_mode arg / render_markdown shorthand; Lisp 真成 dispatch SSOT, Markdown brief 不再 load-bearing (commit 681c95d)."
     "DONE wave20-05 — unified-entry machine-loop smoke v2: 6 smoke tests + build_artifact_refs lift 8 个 machine-contract 字段 + Markdown non-load-bearing 钉死 (commit d308fae)."
     "DONE wave20-06 — cross-plan distill auto-trigger v1: auto_chain_trigger default 'never' / 'deterministic_only' + 6 trigger rule (commit 3669ebc)."
     "DONE wave20-07 — LLM-augmented plan inference v0: infer_plan_fields=sonnet_suggest opt-in / suggest only / applied=false 钉死; DAG mode 拒 (commit 6bb935a). Caveat: 完整 LLM-augmented apply 仍 future."
     "DONE wave20-08 — review auto-answer policy v0: auto_answer_policy off|deterministic_safe|dry_run + 5+2 rules + 3 hard invariants I1 never reject / I2 destructive never auto-promote / I3 no LLM (commit 8adb0a8)."
     "DONE wave20-09 — ExecutionEvent legacy metadata sweep v0: 8 legacy variants 全加 dispatch trio; 11 variants 全闭环 (commit 6e01e3f)."
     ";; wave 21 closures (all done — propose+apply-gate paradigm):"
     "DONE wave21-01 — hooks-path installer v1: scripts/install-missiond-hooks.mjs (--check/--install/--json/--dry-fixture/--strict) + scripts/check-missiond-hooks.mjs read-only doctor alias + .githooks/pre-commit 保 MISSIOND_TASK_CONTRACT env-gated; --install runs git config --local core.hooksPath .githooks once, no-op when aligned, never --global/--system; **opt-in repo-local only — 不擅自 default-on** (commit 44c74df)."
     "DONE wave21-02 — task-run verifier v1: scripts/verify-task-run.mjs 三合一 (task contract + report task_id + commit_hash + memory completion) + 12 dry-fixtures + 7 helper cases + 14 forbidden git verb proof + dogfood self-verify; verify-task-contract.mjs main() gated by import.meta.url so importers don't trigger CLI parsing (commit 1335fa7)."
     "DONE wave21-03 — execution report-verifier integration v1: mission_execution(complete) 加 4 新字段 (task_run_verifier_status / shared_memory_path / verifier_diagnostics / verified) + verified=true 触发 daemon-internal sexp-parse cross-check 加 4 structured error codes (VERIFIED_REQUIRES_ENFORCEMENT/TASK_CONTRACT/TASK_REPORT/COMMIT_HASH + TASK_REPORT_REQUIRED/MALFORMED + TASK_REPORT_TASK_ID_MISMATCH + TASK_REPORT_COMMIT_HASH_MISMATCH + TASK_CONTRACT_MALFORMED) + daemon_never_invokes_mutating_git unit test pinned; daemon NEVER spawns Node — wave21-02 verifier 仍是 out-of-process 权威 (commit 308426e)."
     "DONE wave21-04 — autonomous workstation LLM proposal v0: workstation_inference_mode=off|sonnet_suggest opt-in (default off byte-shape preserved); WorkstationProposalGate 仅在 caller_target/dispatch_strategy/objective/scope/owned_files/project_signal AND plan_hints + plan_workstation_opt_in 全空才 propose; 4 propose 字段 target/dispatch_strategy/objective/scope × 3 confidence (high/medium/low) × 4 safety (Safe/InvalidTarget/InvalidStrategy/UnsupportedTarget); workstation_proposals[] cap 6; **propose only, never auto-spawn — applied=false / auto_spawn=false 永钉死**; Sonnet unavailable status=llm_unavailable + reason 钉 'no fallback to claude -p / prompt mode'; DAG mode preflight rejects sonnet_suggest INVALID_PARAM (commit 68b84f1)."
     "DONE wave21-05 — plan inference apply gate v1: apply_inferred_fields=true opt-in apply gate 接 wave-18/06 deterministic + wave-20/07 LLM proposals; **6 道 gate** (caller_approval / master_flag / confidence ∈ {high, medium} / conflict=none / per-field safety / slot availability) + llm_caller_approved (object/array shape) strict-shape validated; 8 skip reason canonical (apply_gate_not_requested / caller_value_already_set / caller_value_conflict / below_apply_threshold / llm_not_caller_approved / llm_confidence_too_low / llm_conflict_present / llm_safety_check_failed / deterministic_inferred_already_applied); **persisted plan.sexp_text 永不被 mutate** (persist_inference_applied=false 永钉死) — persist_inference flag echoed for future wave (commit a18200b)."
     "DONE wave21-06 — LLM auto-approve proposal v0: auto_approve_mode=off|sonnet_suggest opt-in for directive (approve|archive) + plan (approve|mark|supersede) review surfaces; ORTHOGONAL to wave-18/07 review_automation_policy AND wave-20/08 auto_answer_policy (3 knobs co-exist on response); **5 invariants test-pinned**: I1 NEVER auto-reject (rejected from model demoted to needs_changes + proposal_warnings[]); I2 destructive (archive|supersede|remove case-insensitive) short-circuit destructive_blocked WITHOUT calling Sonnet; I3 applied=false + requires_human=true 永钉死 (propose-only); I4 sonnet unavailable surfaces llm_unavailable + reason, NO fallback to deterministic; I5 destructive_check ALWAYS sourced from is_destructive_review_action(action), overwriting model output via enforce_proposal_invariants helper; 22 unit tests; 10 dispatch branch sites (commit e140773)."
     "DONE wave21-07 — sonnet distill chain auto-apply v1: auto_sonnet=true + auto_sonnet_approved=true 双 opt-in apply-gate 接 wave-20/06 cross-plan distill auto-trigger; **7 重 gate** (双 opt-in + auto_chain_trigger=auto_safe + ALL 6 wave-20 deterministic safety rule + caller distill_mode != sonnet); auto-promote inner distill from dry_run to sonnet via direct call to action_distill_sonnet; **8 status taxonomy**: not_requested | disabled | skipped_no_trigger | skipped_rules_failed | skipped_caller_approval_missing | skipped_already_sonnet | skipped_inner_error | applied_sonnet; **7 invariants test-pinned**: I1 default-off byte-shape; I2 dual opt-in (single typo cannot escalate); I3 reuse wave-20 trigger outcomes never relax; I4 caller-already-sonnet refusal; I5 Sonnet failure preserve inner payload (model_call_status=failed|invalid_output); I6 review_required=true PINNED on every successful auto-apply (receipt-only, no DB transition); I7 wave-19/20 blocks UNCHANGED (purely additive); 16 + 4 unit tests (commit 4d494db)."
     "DONE wave21-08 — machine-contract autonomous loop smoke v3: 15 new deterministic e2e tests across 4 handler files (unified_entry +3 / plan +3 / workstation_dispatch +3 / agent_execution +6); covers wave21-03 verifier 5-rule cross-check happy + 5 structured failure paths; pins wave21-04 I3 + I4 + I5 (workstation_proposals applied=false / auto_spawn=false / unavailable surfaces no fallback); pins wave21-06 I1 + I2 + I3 + I5 (NEVER rejected / destructive_blocked / requires_human=true / destructive_check from helper); pins wave21-07 I1 + I3 + I7 (default-off / chain reuses trigger outcomes / chain block additive); **Markdown task_brief_preview NEVER projected into artifact_refs (non-load-bearing 二度钉死)**; machine dispatch SSOT task_contract_path == task_contract_source_path 钉死; no LLM, no spawn, no shell (commit 8ba8723)."
     ";; wave 22 closures (all done — explicit-gate-promotion + auto-verifier + smoke v4 paradigm):"
     "DONE wave22-01 — hooks default-on doctor v2: install-missiond-hooks.mjs default mode (无 flag) 升级到 = --check 只读 doctor (read-only); --install 仍唯一 mutation entry; doctor JSON payload 加 :severity (ok|preflight-drift), :reason (4 codes: aligned|hooks-path-unset|hooks-path-wrong|hook-file-missing), :advice, :install_command; renderer 加 renderHooksDoctorPreflight() 块在每 :commit :required brief commit section 上方 (check-missiond-hooks --json + install-missiond-hooks --install 双指令); dry-fixture 8→11 covers 4 doctor states + install-refuses-when-hook-file-missing + adviceFor() install-command surface assertions; **opt-in repo-local only — git config 真 mutation 仍 caller 显式 --install** (commit 49555c4)."
     "DONE wave22-02 — execution auto-run-verifier v2: daemon-internal `auto_run_task_run_verifier` 8 cross-checks in-process (task_contract_loadable / task_report_loadable / task_report_schema / task_id_matches_contract / commit_hash_matches_report / shared_memory_loadable / shared_memory_schema / shared_memory_completion_for_task); 当 task_contract_path + task_report_path + shared_memory_path + commit_hash 4 路径全提供时 daemon 自跑 cross-check (read-only file inspection — 绝不 spawn Node/shell/mutating git); verification_source='daemon-auto-verifier' 主路径; legacy verified=true with missing paths 降级 verification_source='legacy-caller-claim' 不硬拒 (back-compat); 3 新错误码 SHARED_MEMORY_REQUIRED / SHARED_MEMORY_MALFORMED / SHARED_MEMORY_NO_COMPLETION_FOR_TASK; wave21-03 enforce_verified_completion helper 仍保留 (legacy verified=true 路径仍 honored); 8 new tests; daemon NEVER spawns Node — wave21-02 verifier 仍 out-of-process 权威 (commit 02ac627)."
     "DONE wave22-03 — review LLM approve apply gate v1: apply_llm_auto_approve=true + proposal_hash (32-hex deterministic SHA-256) + caller_approved=true 4 opt-in (legacy quiet `action=approve` 路径触发 DB transition); **6 道严格 gate**: G1 apply flag G2 proposal_hash deterministic match (compute_proposal_hash 32-hex SHA-256 over action+artifact+version+decision+confidence+destructive prefix) G3 caller_approved=true G4 deterministic non-destructive G5 decision=approved G6 confidence=high; 3 新错误码 APPLY_GATE_MISSING_PROPOSAL_HASH / APPLY_GATE_PROPOSAL_HASH_MISMATCH / APPLY_GATE_INVALID_PARAM (hash mismatch fail-fast BEFORE DB mutation); response surface: `llm_auto_approve_proposal_hash` (always stamped when bundle carries proposal) + `llm_approve_apply_gate` block (apply_status, applied_decision, proposal_hash_status, computed_proposal_hash, supplied_proposal_hash, caller_approved, safety_rule_results[]); explicit review_decision 路径 gate 仍 informational only (no DB mutation); destructive actions (archive/supersede) ALWAYS skip with SkippedDestructiveAction; **wave-21/06 5 invariants 全 preserved 5 dedicated tests** (I1 NeedsChanges skipped non-approved / I2 archive/supersede/remove destructive skip / I3 proposal applied=false BEFORE+AFTER gate / I4 unavailable no fallback / I5 model-lied destructive_check overridden by deterministic verdict) (commit 4b55cb4)."
     "DONE wave22-04 — persisted plan inference apply v2: persist_inference=true + caller_approved=true + proposal_hash + apply_inferred_fields=true 4 opt-in apply gate (在 wave-21/05 v1 之上加 persisted layer); compute_inference_proposal_hash 32-hex SHA-256 over (plan_id, original_sexp_hash, sorted applied_fields); execute_persisted_apply DB path: plan_list_by_task -> plan_insert(next-version) with original sexp 加 (plan-inference-applied :inference-version v2 :proposal-hash ... :persisted-at ...) annotation -> plan_supersede(old_id) -> append_plan_evidence_entry typed source/kind=plan_inference_persisted_apply with rollback_plan_id=predecessor; 2 新错误码 PERSIST_APPLY_MISSING_PROPOSAL_HASH / PERSIST_APPLY_PROPOSAL_HASH_MISMATCH (hash mismatch fail-fast BEFORE DB mutation); attach_persisted_apply_block stable wire surface (mirrors apply gate block); **wave-21/05 6 invariants 全 preserved 7 dedicated tests** (apply_gate_v1_byte_shape_when_off / conflicts_never_persist / suggestions_never_persist / llm_unapproved_never_persists / strict_bool_shape / persist_inference_applied_field_intact / was_applied_only_for_applied); **I6 v1 `apply_gate.persist_inference_applied=false` 仍硬钉死** (v2 用 SEPARATE `persisted_apply` block surface 状态, 不动 v1 字段) (commit fee6567)."
     "DONE wave22-05 — autonomous workstation true spawn v1: auto_spawn=true + workstation_caller_approved=true + preflight_acceptable=true + workstation_proposal_hash 4 opt-in (mode sonnet_suggest 是前置必填); **12-rule gate matrix**: G1 auto_spawn opt-in / G2 bundle Suggested / G3 hash matches (compute_workstation_proposal_hash 32-hex SHA-256 over v1 sentinel + bundle status + each proposal field|value|confidence|safety_status semicolon-joined, evidence text excluded) / G4 ALL proposals safety_status=safe / G5 ALL proposals confidence=high / G6 caller_approved=true / G7 preflight_acceptable=true / G8 task_contract_path supplied / G9 contract loads ok / G10 :write-scope non-empty / G11 :write-scope non-overlap with :must-not-touch / G12 proposed target=mission_task_delegate; 走 `mission_task_delegate` substrate **绝不 `claude -p`** (substrate's existing SafeDescriptorReason::UnsupportedTarget 拒非 mission_task_delegate up-front + G12 也拒); 3 新错误码 AUTO_SPAWN_INVALID_PARAM / AUTO_SPAWN_MISSING_PROPOSAL_HASH / AUTO_SPAWN_PROPOSAL_HASH_MISMATCH; 15 status taxonomy (not_requested|spawned|skipped_unavailable|skipped_no_proposals|skipped_unsafe_proposal|skipped_confidence_too_low|skipped_caller_not_approved|skipped_missing_task_contract_path|skipped_malformed_task_contract|skipped_empty_write_scope|skipped_forbidden_scope_overlap|skipped_preflight_unacceptable|skipped_unsupported_target|skipped_substrate_refused|skipped_substrate_inner_error); **wave-21/04 4 invariants 全 preserved 4 dedicated tests** (default off / Sonnet unavailable no fallback / DAG mode rejects / propose-only fields preserved on wave-21/04 surface) (commit 162a303)."
     "DONE wave22-06 — distill chain policy auto-sonnet v2: auto_sonnet_policy ∈ {off, safe_after_rules, dry_run} closed-enum policy-driven; **dual opt-in 移除** — policy 选择 (单一显式 enum) 即 explicit operator attestation, 不需 auto_sonnet_approved 第二 flag; legacy auto_sonnet=true + auto_sonnet_approved=true 双 opt-in 路径仍 back-compat coexists (additive blocks 同 call 可同时 surface); safe_after_rules 触发要 ALL 6 wave-20 rule pass + trigger=auto_safe + distill_mode != sonnet; dry_run 完整 evaluate 仅 surface 不 spawn Sonnet; 8 AUTO_SONNET_POLICY_STATUS_* wire-string constants (not_requested|off|safe_after_rules_applied|safe_after_rules_dry_run|skipped_no_trigger|skipped_rules_failed|skipped_already_sonnet|skipped_inner_error|invalid_param); strict closed-enum parse rejects 10 typo / camelCase / case-mismatch / shape-mismatch inputs at action entry (single typo cannot escalate); **wave-21/07 7 invariants 全 preserved 7 dedicated tests** (default off byte shape / strict shape no typo escalation / rules must pass no relax / already sonnet refuses double call / Sonnet failure preserves inner / review_required PINNED 7 outcomes / wave-19/20/21 blocks UNCHANGED 4-block coexistence) (commit 2423d4b)."
     "DONE wave22-07 — autonomous loop apply smoke v4: 9 new deterministic smoke tests across 5 in-scope files (unified_entry +1 envelope smoke pinning all 22 cross-wave invariants + markdown non-load-bearing across 10 forbidden artifact_refs keys; agent_execution +2 verifier failure paths SHARED_MEMORY_NO_COMPLETION_FOR_TASK + TASK_REPORT_COMMIT_HASH_MISMATCH; plan +2 persisted apply gate fixture hash accept + missing hash reject + wave-21/05 6 invariants pinned; workstation_dispatch +2 auto-spawn gate fixture hash accept + missing hash reject + wave-21/04 4 invariants pinned; review_gate +2 review apply gate fixture hash accept + missing hash reject + wave-21/06 5 invariants pinned); **22 cross-wave invariants pinned** (wave21-04 4 + wave21-05 6 + wave21-06 5 + wave21-07 7 = 22); no real LLM (synthesised LlmAutoApproveProposalBundle / WorkstationProposalBundle / PlanFieldInference / AppliedField fixtures, NO Sonnet gateway initialized); no real spawn (workstation evaluator is PURE function ending at WorkstationAutoSpawnStatus::Spawned without calling substrate); no mutating git (verifier helpers do read-only file inspection on tempfile-backed paths only) (commit 6b2125c)."
     ";; remaining future:"
     "完全 LLM 自主无任何 caller opt-in (wave22-03/04/05/06 仍 require explicit caller_approval/proposal_hash/policy/auto_spawn opt-in)."
     "Sonnet 真无任何 attestation (wave22-06 policy=safe_after_rules 仍是 explicit policy 选择即 attestation)."
     "git hooks default-on real install (wave22-01 仍 doctor only — caller 必须显式 git config core.hooksPath .githooks 才生效)."
     "Auto-seed shared-memory ledger claim entry on parallel workstation spawn (wave22-05 真 spawn 已落, ledger seed 仍 future)."
     "DONE wave24-01..06 — router-policy dry-run chain: schema/checker/seed + trace corpus indexer + recommendation CLI + mission_plan dry-run surface + renderer advisory block + full-chain smoke; no runtime backend replacement."
     "DONE wave25-01..05 — router-policy measurement loop: corpus evaluator (67 real tasks), report-contract seven router fields, mission_plan trace-index confidence path, renderer recommend command, and CLI/Rust parity smoke; no runtime backend replacement."
     "After backend loop stabilizes, reuse the same contract style for timeline-edit operations."
     "Frontend Lisp 仍 postpone (本 wave 不开)."]))
