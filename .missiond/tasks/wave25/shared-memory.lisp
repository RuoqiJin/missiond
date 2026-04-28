;; Wave 25 shared-memory ledger.
;; Schema: .missiond/tasks/schema/shared-memory-v1.lisp
;;
;; Append-only. Agents add entries while they hold a live :claim for their
;; task id. Editing or removing prior entries is forbidden; append a
;; (correction ...) entry instead.

(shared-memory wave25
  :schema "missiond.shared-memory.v1"
  :wave wave25
  :created-at "2026-04-28T15:00:00+08:00"
  :sequence 1

  (observation
    :id wave25-bootstrap-001
    :task wave25-00-archive-wave24-artifacts
    :agent codex-orchestrator
    :seq 1
    :at "2026-04-28T15:00:00+08:00"
    :touched []
    :summary "Bootstrap entry: Wave 25 turns Wave 24 router dry-run into a measurable policy loop: archive artifacts, evaluate recommendations over the trace corpus, record recommendation fields in reports, and align mission_plan dry-run confidence with trace-index evidence. Runtime backend replacement remains out of scope.")

  (claim
    :id wave25-00-claim-001
    :task wave25-00-archive-wave24-artifacts
    :agent claudecode
    :seq 2
    :at "2026-04-28T15:30:00+08:00"
    :touched [".missiond/tasks/wave24/**"
              ".missiond/claudecode/wave24-*.md"]
    :summary "Claim wave25-00-archive-wave24-artifacts: archive untracked Wave 24 task contracts, rendered briefs, reports, shared-memory.lisp and session-trace.lisp. Honors must-not-touch: crates/** scripts/** .missiond/v2/** .missiond/tasks/wave25/wave25-*.lisp .missiond/claudecode/wave25-*.md.")

  (completion
    :id wave25-00-completion-001
    :task wave25-00-archive-wave24-artifacts
    :agent claudecode
    :seq 3
    :at "2026-04-28T15:36:00+08:00"
    :touched [".missiond/claudecode/wave24-00-archive-wave23-artifacts.md"
              ".missiond/claudecode/wave24-01-router-policy-schema-v1.md"
              ".missiond/claudecode/wave24-02-trace-corpus-index-v0.md"
              ".missiond/claudecode/wave24-03-router-recommendation-cli-v0.md"
              ".missiond/claudecode/wave24-04-plan-router-dry-run-surface-v0.md"
              ".missiond/claudecode/wave24-05-renderer-router-context-v0.md"
              ".missiond/claudecode/wave24-06-router-dry-run-smoke-v0.md"
              ".missiond/claudecode/wave24-07-lisp-backfill-router-dry-run-status.md"
              ".missiond/claudecode/wave24-08-parallel-dispatch-index.md"
              ".missiond/tasks/wave24/reports/wave24-00-archive-wave23-artifacts.report.lisp"
              ".missiond/tasks/wave24/reports/wave24-01-router-policy-schema-v1.report.lisp"
              ".missiond/tasks/wave24/reports/wave24-02-trace-corpus-index-v0.report.lisp"
              ".missiond/tasks/wave24/reports/wave24-03-router-recommendation-cli-v0.report.lisp"
              ".missiond/tasks/wave24/reports/wave24-04-plan-router-dry-run-surface-v0.report.lisp"
              ".missiond/tasks/wave24/reports/wave24-05-renderer-router-context-v0.report.lisp"
              ".missiond/tasks/wave24/reports/wave24-06-router-dry-run-smoke-v0.report.lisp"
              ".missiond/tasks/wave24/session-trace.lisp"
              ".missiond/tasks/wave24/shared-memory.lisp"
              ".missiond/tasks/wave24/wave24-00-archive-wave23-artifacts.lisp"
              ".missiond/tasks/wave24/wave24-01-router-policy-schema-v1.lisp"
              ".missiond/tasks/wave24/wave24-02-trace-corpus-index-v0.lisp"
              ".missiond/tasks/wave24/wave24-03-router-recommendation-cli-v0.lisp"
              ".missiond/tasks/wave24/wave24-04-plan-router-dry-run-surface-v0.lisp"
              ".missiond/tasks/wave24/wave24-05-renderer-router-context-v0.lisp"
              ".missiond/tasks/wave24/wave24-06-router-dry-run-smoke-v0.lisp"
              ".missiond/tasks/wave24/wave24-07-lisp-backfill-router-dry-run-status.lisp"
              ".missiond/tasks/wave24/wave24-08-parallel-dispatch-index.lisp"]
    :summary "Archived 27 Wave 24 artifacts in commit 83b575d9c19d (9 contracts + 9 briefs + 7 reports + shared-memory.lisp + session-trace.lisp). Reports 07-08 intentionally absent: wave24-07 was Codex-owned Lisp backfill committed earlier as bce64e2; wave24-08 is a coordination dispatch index that emits no report. All acceptance commands exit 0.")

  (claim
    :id wave25-01-claim-001
    :task wave25-01-router-policy-corpus-evaluator-v0
    :agent claudecode
    :seq 4
    :at "2026-04-28T16:00:00+08:00"
    :touched ["scripts/evaluate-router-policy-corpus.mjs"]
    :summary "Claim wave25-01-router-policy-corpus-evaluator-v0: add a NEW read-only evaluator CLI that walks the task corpus and runs the recommend() pipeline (imported from scripts/recommend-task-backend.mjs) over each task, building or loading a trace index in-process via buildIndex (imported from scripts/build-session-trace-index.mjs). Read-only: no shell, no git, no LLM, no HTTP. Honors must-not-touch: crates/** scripts/recommend-task-backend.mjs scripts/build-session-trace-index.mjs scripts/check-router-policy.mjs .missiond/v2/** .missiond/tasks/schema/*.lisp .missiond/tasks/wave24/** .missiond/tasks/wave25/wave25-*.lisp .missiond/claudecode/**.")

  (claim
    :id wave25-02-claim-001
    :task wave25-02-report-router-recommendation-fields-v0
    :agent claudecode
    :seq 5
    :at "2026-04-28T16:05:00+08:00"
    :touched [".missiond/tasks/schema/report-contract-v1.lisp"
              "scripts/check-task-report.mjs"]
    :summary "Claim wave25-02-report-router-recommendation-fields-v0: extend report-contract v1 with optional flat router-recommendation fields (recommended_backend, router_confidence, router_policy_path, router_dry_run_only, router_applied, router_reasons, router_trace_index_path), wire optional validators into checker, and add 6 new fixtures (legacy / valid block / invalid backend / applied=true / dry_run_only=false / absolute policy path) bringing total from 16 to 22. Strict additive; backward-compat preserved. Honors must-not-touch: crates/** scripts/render-claudecode-task.mjs scripts/recommend-task-backend.mjs scripts/evaluate-router-policy-corpus.mjs .missiond/v2/** .missiond/tasks/wave24/** .missiond/tasks/wave25/wave25-*.lisp .missiond/claudecode/**.")

  (claim
    :id wave25-03-claim-001
    :task wave25-03-plan-router-trace-index-confidence-v1
    :agent claudecode
    :seq 6
    :at "2026-04-28T16:10:00+08:00"
    :touched ["crates/missiond-daemon/src/handlers/knowledge/plan.rs"
              "crates/missiond-mcp/src/tools/knowledge/plan.rs"]
    :summary "Claim wave25-03-plan-router-trace-index-confidence-v1: extend mission_plan execute schema with OPTIONAL router_policy_trace_index_path; under router_policy_mode=dry_run, daemon reads the trace-index JSON via std::fs + serde_json and uses by_task[task_id].events / by_backend[recommended_backend].events for confidence scoring (matched + max(events) >= 5 => high; 1..4 => medium; 0 => low). Off/default mode stays byte-identical with NO file I/O. Adds router_recommendation fields trace_index_path / trace_index_status / trace_index_warning. Failure modes (missing/unreadable/malformed) surface trace_index_status without failing dispatch. applied=false stays a hard-coded literal. New tests pin: off-with-path-no-io / dry_run high|medium|low / missing|malformed|absent statuses / dispatch unchanged / applied=false invariant. Honors must-not-touch: workstation_dispatch.rs / agent_execution.rs / unified_entry.rs / plan_dag.rs / scripts/** / .missiond/v2/** / .missiond/tasks/**.")

  (completion
    :id wave25-02-completion-001
    :task wave25-02-report-router-recommendation-fields-v0
    :agent claudecode
    :seq 7
    :at "2026-04-28T16:21:00+08:00"
    :touched [".missiond/tasks/schema/report-contract-v1.lisp"
              "scripts/check-task-report.mjs"]
    :summary "Committed wave25-02 in 770903136f00 (feat(tasks): record router recommendation in reports). Surface: Option A flat top-level keys. Schema additions: 7 optional fields (:recommended_backend / :router_confidence / :router_policy_path / :router_dry_run_only / :router_applied / :router_reasons / :router_trace_index_path) listed in optional-report-fields, documented in field-contract, and reflected in checker-contract :rejects. Checker additions: 4 helpers (validateRouterEnumField / validateRouterRepoRelativePath / validateRouterLiteralBool / validateRouterReasons) plus 6 new fixtures bringing total from 16 -> 22. Backward-compat: legacy reports (no router fields) validate cleanly; the existing 16 fixtures remain byte-identically green; --all run validates all 43 existing reports unchanged. Acceptance results — node scripts/check-task-report.mjs --dry-fixture: exit 0 (22/22); node scripts/check-task-report.mjs .missiond/tasks/wave24/reports/wave24-04-plan-router-dry-run-surface-v0.report.lisp: exit 0; node scripts/check-task-contract.mjs --all: exit 0 (74 tasks); git diff --check on the two staged files: exit 0. task-scope-guard --mode staged: 2 staged files, 0 must-not-touch matches. verify-task-contract: OK against 770903136f00.")

  (completion
    :id wave25-01-completion-001
    :task wave25-01-router-policy-corpus-evaluator-v0
    :agent claudecode
    :seq 8
    :at "2026-04-28T16:26:00+08:00"
    :touched ["scripts/evaluate-router-policy-corpus.mjs"]
    :summary "Committed wave25-01 in 8dbe85fa1a0c (feat(tasks): evaluate router policy over trace corpus). Single new file (1013 LOC) implementing scripts/evaluate-router-policy-corpus.mjs — a read-only corpus evaluator that imports the wave24-02 / wave24-03 / wave23-06 named exports (recommend / readTaskContractFile / FALLBACK_BACKEND / FALLBACK_REASON / buildIndex / findSessionTraceFiles / DEFAULT_SCAN_ROOT / BACKEND_CLASSES / readRouterPolicyFile / parseTraceEvents) and never spawns the underlying CLIs. Schema missiond.router-policy-evaluation.v0 with totals / by_backend (zero-seeded against wave24-01 BACKEND_CLASSES enum) / by_confidence / fallback_count / rejected_count / per_task (sorted by task_id) / policy_path / tasks_root / trace_index_source. Corpus walk skips schema/, reports/, shared-memory.lisp, session-trace.lisp, and *-parallel-dispatch-index.lisp. 8 dry-fixture cases pass: empty-corpus, multi-task-corpus, fallback-rows, rejected-policy (runtime-replacement true), rejected-task (malformed contract), deterministic-output, trace-index-equivalence (built vs supplied), self-audit (no shell / git / LLM / HTTP). Real corpus run: 67 tasks evaluated, 43 fallbacks, 0 rejections; by_backend = {claudecode:49, deterministic-checker:14, verifier-worker:4, missiond-llm-router:0, patch-worker:0}; by_confidence = {high:6, medium:5, low:56}. Acceptance — node scripts/evaluate-router-policy-corpus.mjs --dry-fixture: exit 0 (8/8 OK, 8 categories); node scripts/evaluate-router-policy-corpus.mjs --policy .missiond/router/router-policy-v1.lisp --tasks-root .missiond/tasks --json: exit 0; node scripts/check-router-policy.mjs .missiond/router/router-policy-v1.lisp: exit 0; node scripts/check-task-contract.mjs --all: exit 0 (74 tasks); git diff --check -- scripts/evaluate-router-policy-corpus.mjs: exit 0. task-scope-guard --mode staged: 1 staged file, 0 must-not-touch matches. verify-task-contract: OK against 8dbe85fa1a0c.")

  (completion
    :id wave25-03-completion-001
    :task wave25-03-plan-router-trace-index-confidence-v1
    :agent claudecode
    :seq 9
    :at "2026-04-28T16:36:00+08:00"
    :touched ["crates/missiond-daemon/src/handlers/knowledge/plan.rs"
              "crates/missiond-mcp/src/tools/knowledge/plan.rs"]
    :summary "Committed wave25-03 in bd2b5a34fd03 (feat(plan): score router dry-run with trace index). MCP arg added: router_policy_trace_index_path (string, optional, default absent). Behavior: under router_policy_mode=dry_run AND path supplied, daemon reads trace-index JSON via std::fs::read_to_string + serde_json and computes confidence using by_task[plan.board_task_id].events / by_backend[recommended_backend].events; matched + max(events) >= 5 => high; 1..=4 => medium; 0 => low; no-match always low. Mirrors scripts/recommend-task-backend.mjs scoreConfidence (RICH_TRACE_THRESHOLD=5). Off/default mode does NO file I/O even with path supplied — early-return in attach_router_recommendation_block guards compute_recommendation. New response fields on router_recommendation block: trace_index_path (echo) / trace_index_status (used | missing | unreadable | malformed) / trace_index_warning (one-line, only when degraded). When path arg is absent, ALL three fields are OMITTED entirely (preserves wave24-04 byte-shape). Failure handling is non-fatal across the board: missing -> std::io::ErrorKind::NotFound; unreadable -> any other I/O error; malformed -> serde_json error or non-object top-level shape; matched-with-degraded-trace falls back to medium. applied=false stays a hard-coded literal in EVERY emitted block (re-pinned by applied_remains_false_with_trace_index across used / missing / malformed / absent). 9 new tests under handlers::knowledge::plan::tests. Acceptance: cargo test -p missiond-daemon: 1646 / 0 (was 1637 baseline); cargo test -p missiond-mcp --lib: 17 / 0 (matches baseline); cargo build --workspace: clean; node scripts/check-task-contract.mjs --all: 74 tasks OK; git diff --check on the two staged files: clean; task-scope-guard --mode staged: 2 staged files, 0 must-not-touch matches. verify-task-contract: OK against bd2b5a34fd03.")

  (claim
    :id wave25-04-claim-001
    :task wave25-04-renderer-router-recommendation-command-v1
    :agent claudecode
    :seq 10
    :at "2026-04-28T16:45:00+08:00"
    :touched ["scripts/render-claudecode-task.mjs"
              ".missiond/tasks/schema/task-contract-v1.lisp"]
    :summary "Claim wave25-04-renderer-router-recommendation-command-v1: extend renderer's existing wave24-05 Router Policy (advisory) section with two read-only command lines (node scripts/check-router-policy.mjs <policy-path> and node scripts/recommend-task-backend.mjs --task <THIS_TASK_LISP> --policy <policy-path> --json) parameterized by the resolved policy path AND the task's source path; extend wave23-02 Report Contract section with an advisory MAY-language note about the wave25-02 optional router fields (:recommended_backend / :router_confidence / :router_policy_path / :router_dry_run_only / :router_applied / :router_reasons / :router_trace_index_path). Recommendation stays ADVISORY — preserve literals 'advisory' and 'dry-run only'; never instruct backend switch; never shell out from renderer. Honors must-not-touch: crates/** scripts/check-task-report.mjs scripts/recommend-task-backend.mjs scripts/evaluate-router-policy-corpus.mjs .missiond/tasks/schema/report-contract-v1.lisp .missiond/v2/** .missiond/tasks/wave24/** .missiond/tasks/wave25/wave25-*.lisp .missiond/claudecode/**. task-contract-v1.lisp may be edited only to document the renderer's new behaviour (no new field required — derive from existing :router-policy-path and the task source path).")

  (completion
    :id wave25-04-completion-001
    :task wave25-04-renderer-router-recommendation-command-v1
    :agent claudecode
    :seq 11
    :at "2026-04-28T16:56:00+08:00"
    :touched ["scripts/render-claudecode-task.mjs"
              ".missiond/tasks/schema/task-contract-v1.lisp"]
    :summary "Committed wave25-04 in e1fdbe4a68d0 (feat(tasks): render router recommendation commands). Renderer changes: renderRouterPolicy() signature grew to (lines, task, routerPolicyPath, relSource); the existing 'Inspect the policy with the read-only checker' fenced block (check-router-policy.mjs) is unchanged byte-identical, and a SECOND fenced block was appended after it carrying `node scripts/recommend-task-backend.mjs --task <relSource> --policy <routerPolicyPath> --json` preceded by a one-line MAY-language preamble re-asserting 'advisory' + 'dry-run only' + 'MUST NOT switch backend'. renderReportContract() got a new bullet group enumerating all 7 wave25-02 optional router-recommendation report fields (recommended_backend enum / router_confidence enum / router_policy_path repo-relative / router_dry_run_only literal true / router_applied literal false / router_reasons vector of non-empty strings / router_trace_index_path repo-relative) with MAY-language preamble. Schema changes: task-contract-v1.lisp updated in 3 places (schema-level :status string, field-contract :router-policy-path entry adds wave25-04 sentence, renderer-contract :machine-context-rendered + :backward-compatibility lists grow by 2 entries each). No new contract fields required — derived from existing :router-policy-path and the task source path the renderer is already processing. Backward compat verified: Session Trace section diffs byte-identical between previously committed wave25-01 brief and freshly rendered /tmp output. No shell-out introduced (rg child_process|spawn|exec returns only literal text-string mentions inside advisory bullets). Acceptance results — node scripts/render-claudecode-task.mjs --stdout wave25-01 > /tmp: exit 0; rg 5-patterns hits all 5 (Router Policy at line 114, advisory at lines 84/114/118/128, dry-run only at 84/118/128, recommend-task-backend at 29/131, check-router-policy at 31/52/125); node scripts/check-task-contract.mjs --all: exit 0 (74 tasks); node scripts/check-task-report.mjs --dry-fixture: exit 0 (22/22 — wave25-02 fixtures unchanged); git diff --check on the two staged files: exit 0; node scripts/check-missiond-hooks.mjs --json: severity ok, reason aligned. task-scope-guard --mode staged: 2 staged files, 0 must-not-touch matches. verify-task-contract: OK against e1fdbe4a68d0.")

  (claim
    :id wave25-05-claim-001
    :task wave25-05-router-policy-measurement-smoke-v1
    :agent claudecode
    :seq 12
    :at "2026-04-28T17:00:00+08:00"
    :touched ["scripts/recommend-task-backend.mjs"
              "scripts/evaluate-router-policy-corpus.mjs"
              "scripts/check-task-report.mjs"
              "crates/missiond-daemon/src/handlers/knowledge/plan.rs"]
    :summary "Claim wave25-05-router-policy-measurement-smoke-v1: add cross-layer SMOKE coverage proving the Wave25 measurable router loop stays advisory across evaluator + report fields + renderer commands + mission_plan trace-index confidence. Layer A (Node): extend recommend-task-backend.mjs --dry-fixture with a wave25-05 case wiring synthetic trace-index + 2-rule policy + docs task and asserting parity equality (Node `recommend()` confidence equals what the daemon would compute for the SAME task_events / backend_events shape — high when max>=5, medium when 1..4, low when 0). Extend evaluate-router-policy-corpus.mjs --dry-fixture with a wave25-05 cross-layer-pin case asserting the schema-level cross-wave invariants (runtime-replacement false, dry-run-only true, applied=false NOT a field of the corpus evaluator schema by design — it lives only on the per-recommendation v0 schema; rejected_count goes up when a runtime-replacement policy is fed). Layer B (Rust): add 2 new tests to handlers::knowledge::plan::tests — router_policy_dry_run_smoke_pins_wave25_invariants pins ALL 8 invariants in a single fixture (runtime_replacement=false on policy / dry_run_only=true on policy / applied=false literal on block / recommended_backend ∈ enum / dispatch fields byte-identical / trace_index_status=used / recommended_backend matches a hard-coded expected backend that Layer A also expects); router_policy_cli_rust_parity_for_high_confidence_match documents the CLI/Rust parity for the same fixture inline and asserts daemon emits high+claudecode for the (5,5)-event fixture so the cross-layer parity is provable without shelling out. Layer C (Node): add 1 positive fixture to check-task-report.mjs --dry-fixture (router_dry_run_only=true / router_applied=false / recommended_backend=deterministic-checker / router_confidence=high) so 22 -> 23. Honors must-not-touch: workstation_dispatch.rs / agent_execution.rs / unified_entry.rs / plan_dag.rs / .missiond/v2/** / .missiond/tasks/schema/*.lisp / .missiond/tasks/wave24/** / .missiond/tasks/wave25/wave25-*.lisp / .missiond/claudecode/**.")

  (completion
    :id wave25-05-completion-001
    :task wave25-05-router-policy-measurement-smoke-v1
    :agent claudecode
    :seq 13
    :at "2026-04-28T17:11:00+08:00"
    :touched ["scripts/recommend-task-backend.mjs"
              "scripts/evaluate-router-policy-corpus.mjs"
              "scripts/check-task-report.mjs"
              "crates/missiond-daemon/src/handlers/knowledge/plan.rs"]
    :summary "Committed wave25-05 in 0f5d857faaa8 (test(router): smoke measurable dry-run policy loop). Layers landed: A + B + C (Layer D renderer literal-pin skipped — wave25-04's renderer literals are already cross-asserted by wave24-06's existing brief-pattern smoke). Layer A counts: scripts/recommend-task-backend.mjs --dry-fixture 11 -> 12; scripts/evaluate-router-policy-corpus.mjs --dry-fixture 8 -> 9. Layer B: handlers::knowledge::plan::tests 355 -> 357 (+2). Layer C: scripts/check-task-report.mjs --dry-fixture 22 -> 23. Cross-wave invariants pinned by wave25-05 (8 total): (1) policy runtime_replacement=false re-checked by daemon's reject branch — surfaces as status=computed; (2) policy dry_run_only=true re-checked similarly; (3) applied=Bool(false) JSON literal in EVERY emitted block, type-checked; (4) renderer 'advisory'+'dry-run only' literals — already pinned by wave24-06 brief-pattern smoke + wave25-04 renderer; (5) report-checker rejects router_applied=true / router_dry_run_only=false — pinned by wave25-02 fixtures, untouched here; (6) dispatch byte-shape unchanged for mode=off-with-trace-supplied — daemon test directly asserts byte-identical text; (7) CLI/Rust parity — daemon emits high+claudecode for (5,5)-event docs-task fixture matching Node CLI's wave25-05-parity fixture's expected output; (8) zero shell-out / LLM / git mutation / network in active router code path — Layer B audit scans plan.rs for forbidden std::process::Command / tokio::process / reqwest:: / hyper::Client / openai_api / anthropic_api with strings assembled from parts (wave24-06 / wave25-01 self-audit lesson). Acceptance results — node scripts/evaluate-router-policy-corpus.mjs --dry-fixture: exit 0 (9/9 OK); node scripts/recommend-task-backend.mjs --dry-fixture: exit 0 (12/12 OK); node scripts/check-task-report.mjs --dry-fixture: exit 0 (23/23 OK); cargo test -p missiond-daemon handlers::knowledge::plan::tests: 357/0 passing; cargo test -p missiond-daemon: 1648/0 (was 1646 baseline; +2); cargo build --workspace: clean; git diff --check on 4 staged files: clean; node scripts/check-missiond-hooks.mjs --json: severity ok, reason aligned. task-scope-guard --mode staged: 4 staged files, 0 must-not-touch matches. verify-task-contract: OK against 0f5d857faaa8."))

