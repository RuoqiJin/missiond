;; Wave 25 session trace.
;; Schema: .missiond/tasks/schema/session-trace-v1.lisp

(session-trace wave25
  :schema "missiond.session-trace.v1"
  :wave wave25
  :created-at "2026-04-28T15:00:00+08:00"
  :sequence 1

  (trace-event
    :id wave25-trace-bootstrap-001
    :seq 1
    :at "2026-04-28T15:00:00+08:00"
    :task wave25-00-archive-wave24-artifacts
    :backend codex-orchestrator
    :kind observation
    :summary "Wave 25 begins: router-policy remains dry-run/advisory; this wave measures recommendations, adds report fields, and improves parity between Node CLI and mission_plan dry-run confidence.")

  (trace-event
    :id wave25-00-trace-start-001
    :seq 2
    :at "2026-04-28T15:30:00+08:00"
    :task wave25-00-archive-wave24-artifacts
    :backend claudecode
    :kind start
    :summary "Begin archiving Wave 24 artifacts: 9 task contracts, 9 rendered briefs, 7 reports, shared-memory.lisp, session-trace.lisp.")

  (trace-event
    :id wave25-00-trace-commit-001
    :seq 3
    :at "2026-04-28T15:35:00+08:00"
    :task wave25-00-archive-wave24-artifacts
    :backend claudecode
    :kind commit
    :summary "Committed wave24 archive: 27 files (9 contracts + 9 briefs + 7 reports + shared-memory.lisp + session-trace.lisp). Reports 07-08 intentionally absent (wave24-07 is Codex-owned Lisp backfill committed as bce64e2; wave24-08 is a coordination index that emits no report)."
    :commit_hash "83b575d9c19d"
    :files [".missiond/claudecode/wave24-00-archive-wave23-artifacts.md"
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
            ".missiond/tasks/wave24/wave24-08-parallel-dispatch-index.lisp"])

  (trace-event
    :id wave25-00-trace-complete-001
    :seq 4
    :at "2026-04-28T15:36:00+08:00"
    :task wave25-00-archive-wave24-artifacts
    :backend claudecode
    :kind complete
    :summary "Wave 24 artifact archival complete; verify-task-contract OK against 83b575d9c19d.")

  (trace-event
    :id wave25-01-trace-start-001
    :seq 5
    :at "2026-04-28T16:00:00+08:00"
    :task wave25-01-router-policy-corpus-evaluator-v0
    :backend claudecode
    :kind start
    :summary "Begin scripts/evaluate-router-policy-corpus.mjs: read-only corpus evaluator that imports recommend() / buildIndex / readRouterPolicyFile / parseTraceEvents and aggregates totals/by_backend/by_confidence/fallback_count/rejected_count/per_task into the missiond.router-policy-evaluation.v0 schema. No shell, no git, no LLM.")

  (trace-event
    :id wave25-02-trace-start-001
    :seq 6
    :at "2026-04-28T16:05:00+08:00"
    :task wave25-02-report-router-recommendation-fields-v0
    :backend claudecode
    :kind start
    :summary "Begin extending report-contract v1 with optional flat router-recommendation fields and corresponding checker validators. Surface choice: Option A (flat top-level keys) per brief recommendation. Fields: :recommended_backend (enum), :router_confidence (enum), :router_policy_path (string repo-relative), :router_dry_run_only (literal true), :router_applied (literal false), :router_reasons (vector of non-empty strings), :router_trace_index_path (string repo-relative). Cross-wave invariants: applied=false and dry_run_only=true literal; absolute or ~/.. paths rejected; unknown enum values rejected. Six new fixtures bring fixture count from 16 to 22; existing 16 must remain byte-identically green.")

  (trace-event
    :id wave25-03-trace-start-001
    :seq 7
    :at "2026-04-28T16:10:00+08:00"
    :task wave25-03-plan-router-trace-index-confidence-v1
    :backend claudecode
    :kind start
    :summary "Begin wave25-03: add optional router_policy_trace_index_path arg to mission_plan execute; daemon mirrors Node CLI's scoreConfidence (matched + max(task_events, backend_events) >= 5 => high; 1..4 => medium; 0 => low) using by_task[task_id].events / by_backend[recommended_backend].events from a serde_json-parsed trace-index file. NO file I/O when mode=off (even if path supplied). Adds trace_index_path / trace_index_status / trace_index_warning to the recommendation block; missing|unreadable|malformed surface status without failing dispatch. applied=false hard-coded literal preserved. New tests cover: off-with-path-no-io (byte-identical baseline), dry_run high/medium/low confidence with fixture index, missing/malformed/absent statuses, dispatch byte-identical with vs without trace-index, applied=false invariant. Edits restricted to plan.rs (daemon) + plan.rs (MCP).")

  (trace-event
    :id wave25-02-trace-commit-001
    :seq 8
    :at "2026-04-28T16:20:00+08:00"
    :task wave25-02-report-router-recommendation-fields-v0
    :backend claudecode
    :kind commit
    :summary "Committed wave25-02 router-recommendation report fields (Option A flat surface). Schema: 7 new optional fields documented in report-contract-v1.lisp with cross-wave invariants. Checker: validateRouterEnumField / validateRouterRepoRelativePath / validateRouterLiteralBool / validateRouterReasons helpers added. Fixtures: 16 -> 22 (6 new: legacy / valid block / invalid backend / applied=true / dry_run_only=false / absolute policy path); all 16 prior fixtures byte-identically green. Acceptance: --dry-fixture 22/22 OK; wave24-04 legacy report still validates; check-task-contract --all 74 tasks OK; git diff --check clean."
    :commit_hash "770903136f00"
    :files [".missiond/tasks/schema/report-contract-v1.lisp"
            "scripts/check-task-report.mjs"])

  (trace-event
    :id wave25-02-trace-complete-001
    :seq 9
    :at "2026-04-28T16:21:00+08:00"
    :task wave25-02-report-router-recommendation-fields-v0
    :backend claudecode
    :kind complete
    :summary "wave25-02 complete: report-contract v1 now records router dry-run recommendation observationally (never authoritative). verify-task-contract OK against 770903136f00. The 7 new optional fields preserve byte-identical wave-15..23 report shape when omitted; reports declaring router_applied=true or router_dry_run_only=false are rejected as cross-wave invariant violations.")

  (trace-event
    :id wave25-01-trace-commit-001
    :seq 10
    :at "2026-04-28T16:25:00+08:00"
    :task wave25-01-router-policy-corpus-evaluator-v0
    :backend claudecode
    :kind commit
    :summary "Committed scripts/evaluate-router-policy-corpus.mjs (1013 LOC, single new file). Read-only corpus evaluator: imports recommend / readTaskContractFile / FALLBACK_BACKEND / FALLBACK_REASON / buildIndex / findSessionTraceFiles / DEFAULT_SCAN_ROOT / BACKEND_CLASSES / readRouterPolicyFile / parseTraceEvents via named exports; never spawns the underlying CLIs. Schema missiond.router-policy-evaluation.v0 with totals / by_backend (zero-seeded against BACKEND_CLASSES) / by_confidence / fallback_count / rejected_count / per_task (sorted by task_id) / policy_path / tasks_root / trace_index_source. 8 dry-fixture cases pass: empty-corpus, multi-task-corpus, fallback-rows, rejected-policy (runtime-replacement true), rejected-task (malformed contract), deterministic-output, trace-index-equivalence (built vs supplied), self-audit (no shell / git / LLM / HTTP)."
    :commit_hash "8dbe85fa1a0c"
    :files ["scripts/evaluate-router-policy-corpus.mjs"])

  (trace-event
    :id wave25-01-trace-complete-001
    :seq 11
    :at "2026-04-28T16:26:00+08:00"
    :task wave25-01-router-policy-corpus-evaluator-v0
    :backend claudecode
    :kind complete
    :summary "wave25-01 complete: real corpus run (--policy .missiond/router/router-policy-v1.lisp --tasks-root .missiond/tasks --json) exits 0 with 67 tasks evaluated, 43 fallbacks, 0 rejections; by_backend = claudecode 49 / deterministic-checker 14 / verifier-worker 4 / missiond-llm-router 0 / patch-worker 0 (sums to 67); by_confidence = high 6 / medium 5 / low 56 (sums to 67). verify-task-contract OK against 8dbe85fa1a0c. Trace index built in-process from .missiond/tasks (no --trace-index supplied).")

  (trace-event
    :id wave25-03-trace-commit-001
    :seq 12
    :at "2026-04-28T16:35:00+08:00"
    :task wave25-03-plan-router-trace-index-confidence-v1
    :backend claudecode
    :kind commit
    :summary "Committed wave25-03 in bd2b5a34fd03: mission_plan execute now accepts OPTIONAL router_policy_trace_index_path. Under router_policy_mode=dry_run, daemon reads the trace-index JSON via std::fs::read_to_string + serde_json and uses by_task[plan.board_task_id].events / by_backend[recommended_backend].events to score confidence (matched + max(events) >= 5 => high; 1..=4 => medium; 0 => low) — direct mirror of scripts/recommend-task-backend.mjs scoreConfidence (RICH_TRACE_THRESHOLD=5). Off/default mode does NO file I/O even when path supplied (early-return in attach_router_recommendation_block before compute_recommendation runs). Recommendation block extended with trace_index_path / trace_index_status (used | missing | unreadable | malformed) / trace_index_warning. When path arg is absent, all three fields are OMITTED entirely (preserves wave24-04 byte-shape). Failure modes are non-fatal: missing -> std::io::ErrorKind::NotFound branch; unreadable -> any other I/O error; malformed -> serde_json parse error or non-object top-level shape. Matched-with-degraded-trace falls back to medium confidence; no-match always low. applied=false stays a hard-coded literal in EVERY emitted block (re-pinned by applied_remains_false_with_trace_index across all 4 status flavours)."
    :commit_hash "bd2b5a34fd03"
    :files ["crates/missiond-daemon/src/handlers/knowledge/plan.rs"
            "crates/missiond-mcp/src/tools/knowledge/plan.rs"])

  (trace-event
    :id wave25-03-trace-complete-001
    :seq 13
    :at "2026-04-28T16:36:00+08:00"
    :task wave25-03-plan-router-trace-index-confidence-v1
    :backend claudecode
    :kind complete
    :summary "wave25-03 complete: 9 new tests added under handlers::knowledge::plan::tests (router_policy_mode_off_with_trace_index_supplied_does_no_file_io / router_policy_mode_dry_run_with_trace_index_high_confidence / _medium_confidence / _low_confidence_when_zero_events / _with_missing_trace_index_emits_status_missing / _with_malformed_trace_index_emits_status_malformed / _no_trace_index_supplied_emits_status_absent / _with_trace_index_does_not_change_dispatch / applied_remains_false_with_trace_index). cargo test -p missiond-daemon: 1646 / 0 (was 1637 baseline; +9). cargo test -p missiond-mcp --lib: 17 / 0 (matches baseline). cargo build --workspace: clean. node scripts/check-task-contract.mjs --all: 74 tasks OK. git diff --check: clean. task-scope-guard --mode staged: 2 staged files OK. verify-task-contract: OK against bd2b5a34fd03.")

  (trace-event
    :id wave25-04-trace-start-001
    :seq 14
    :at "2026-04-28T16:45:00+08:00"
    :task wave25-04-renderer-router-recommendation-command-v1
    :backend claudecode
    :kind start
    :summary "Begin wave25-04: extend scripts/render-claudecode-task.mjs renderRouterPolicy() helper with two read-only command lines (node scripts/check-router-policy.mjs <policy-path> AND node scripts/recommend-task-backend.mjs --task <THIS_TASK_LISP> --policy <policy-path> --json) parameterized by both the resolved policy path AND the current task's source path; extend renderReportContract() helper with a brief MAY-language note pointing at the wave25-02 optional router fields (:recommended_backend / :router_confidence / :router_policy_path / :router_dry_run_only / :router_applied / :router_reasons / :router_trace_index_path). Recommendation stays ADVISORY — preserve literals 'advisory' and 'dry-run only'; never instruct backend switch; never shell out from renderer. Plan: pass relSource through renderRouterPolicy; append second fenced block after the existing check-router-policy fenced block; append a one-bullet MAY note + the 7 field names enumerated as a child sub-list to renderReportContract; update task-contract-v1.lisp renderer-contract / backward-compatibility prose to reflect the new commands and report-contract note. Backward-compat: tasks WITHOUT a resolvable router policy continue to render byte-identical (no new fields appear); the wave23-02 Session Trace section is unchanged byte-identically; no shell-out added. Test corpus: render wave25-01 brief to /tmp and rg the 5 patterns Router Policy / advisory / dry-run only / recommend-task-backend / check-router-policy.")

  (trace-event
    :id wave25-04-trace-commit-001
    :seq 15
    :at "2026-04-28T16:55:00+08:00"
    :task wave25-04-renderer-router-recommendation-command-v1
    :backend claudecode
    :kind commit
    :summary "Committed wave25-04 in e1fdbe4a68d0 (feat(tasks): render router recommendation commands). renderRouterPolicy() now takes a 4th argument relSource and appends a second fenced block AFTER the existing check-router-policy block, preceded by a one-line MAY-language preamble: 'You **may** also inspect the dry-run recommendation for THIS task by running the recommendation CLI directly. The renderer never executes it — the command below is rendered text only and stays **advisory** and **dry-run only**; the recommendation does not change dispatch and you MUST NOT switch backend on the strength of its output:' followed by `node scripts/recommend-task-backend.mjs --task <relSource> --policy <routerPolicyPath> --json`. renderReportContract() got a new bullet group enumerating the wave25-02 optional router-recommendation fields with MAY-language preamble and the 7 field names as nested bullets including their type/enum constraints (recommended_backend enum / router_confidence enum / router_policy_path repo-relative / router_dry_run_only literal true / router_applied literal false / router_reasons vector of non-empty strings / router_trace_index_path repo-relative). task-contract-v1.lisp updated in 3 places: schema-level :status string adds wave25-04 note; field-contract :router-policy-path entry adds wave25-04 sentence about the new commands; renderer-contract :machine-context-rendered list grows by 2 entries (router-policy commands + report-contract router-fields note); :backward-compatibility list grows by 2 entries (commands extend in place / note extends in place). All 5 acceptance commands green: render-to-/tmp + rg-5-patterns + check-task-contract --all (74 tasks) + check-task-report --dry-fixture (22/22) + git diff --check clean. task-scope-guard --mode staged: 2 staged files OK. verify-task-contract: OK against e1fdbe4a68d0."
    :commit_hash "e1fdbe4a68d0"
    :files [".missiond/tasks/schema/task-contract-v1.lisp"
            "scripts/render-claudecode-task.mjs"])

  (trace-event
    :id wave25-04-trace-complete-001
    :seq 16
    :at "2026-04-28T16:56:00+08:00"
    :task wave25-04-renderer-router-recommendation-command-v1
    :backend claudecode
    :kind complete
    :summary "wave25-04 complete: renderer now surfaces the router-policy recommendation surface as copy-paste read-only commands while keeping the recommendation strictly advisory. Section ordering preserved: Machine Contract -> Goal -> Ownership -> Must Not Touch -> Requirements -> Acceptance Commands -> Shared Memory -> Report Contract (now with optional router report-fields bullet group) -> Session Trace -> Router Policy (now with the two read-only commands) -> Commit -> Report. wave23-02 Session Trace section diffs byte-identical against the previously committed wave25-01 brief; wave24-05 'advisory' / 'dry-run only' literals preserved; no shell-out introduced (rg child_process|spawn|exec returns only literal text-string occurrences in advisory bullets); the rendered recommend-task-backend command line for wave25-01 is `node scripts/recommend-task-backend.mjs --task .missiond/tasks/wave25/wave25-01-router-policy-corpus-evaluator-v0.lisp --policy .missiond/router/router-policy-v1.lisp --json`. verify-task-contract: OK against e1fdbe4a68d0.")

  (trace-event
    :id wave25-05-trace-start-001
    :seq 17
    :at "2026-04-28T17:00:00+08:00"
    :task wave25-05-router-policy-measurement-smoke-v1
    :backend claudecode
    :kind start
    :summary "Begin wave25-05: cross-layer SMOKE pinning the Wave25 measurable router loop. Layer A: extend recommend-task-backend.mjs --dry-fixture with a wave25-05 case asserting CLI/Rust confidence parity using the SAME synthetic trace-index shape both engines consume (`by_task[task_id].events` + `by_backend[backend].events`); the Node CLI's scoreConfidence MUST agree with the Rust daemon's confidence selection at high (5+ events) AND medium (1..4) AND low (0). Layer A also extends evaluate-router-policy-corpus.mjs --dry-fixture with a cross-layer invariant pin: build a corpus of 3 tasks across 3 backends + a runtime-replacement policy, assert evaluator REJECTS the runtime-replacement policy (rejected_count >= 1 OR error exit), and that totals.tasks matches the corpus row count when the policy is well-formed. Layer B: add 2 daemon tests in handlers::knowledge::plan::tests — router_policy_dry_run_smoke_pins_wave25_invariants pins ALL 8 cross-wave invariants in one shot (1: policy.runtime_replacement=false re-checked on parsed temp policy; 2: policy.dry_run_only=true; 3: applied=false JSON Bool literal; 4: renderer 'advisory'+'dry-run only' literal in source; 5: report-checker rejects router_applied=true (already pinned by wave25-02 fixtures, daemon test cross-references count); 6: dispatch byte-identical with mode=off-with-trace-supplied; 7: CLI/Rust parity — daemon emits high for (5,5) trace-index just like Node CLI; 8: NO shell-out / LLM / git mutation in router code path); router_policy_cli_rust_parity_for_high_confidence_match drives a (5,5) fixture and asserts confidence=high + recommended_backend=claudecode (matching Node CLI's wave25-05 fixture inline-documented expectation). Layer C: add 1 wave25-05 positive fixture to check-task-report.mjs --dry-fixture (deterministic-checker + high + applied=false + dry_run_only=true) so 22 -> 23. NOT touching: workstation_dispatch.rs / agent_execution.rs / unified_entry.rs / plan_dag.rs / .missiond/v2/** / .missiond/tasks/wave24/** / .missiond/tasks/wave25/wave25-*.lisp / .missiond/claudecode/**.")

  (trace-event
    :id wave25-05-trace-commit-001
    :seq 18
    :at "2026-04-28T17:10:00+08:00"
    :task wave25-05-router-policy-measurement-smoke-v1
    :backend claudecode
    :kind commit
    :summary "Committed wave25-05 in 0f5d857faaa8 (test(router): smoke measurable dry-run policy loop). Layer A (Node): scripts/recommend-task-backend.mjs --dry-fixture grew from 11 -> 12 cases (added wave25-05-parity: 3 confidence buckets high/medium/low pinned for the same docs->claudecode rule shape, plus deterministic-checker bucket parity for the Layer C positive fixture). scripts/evaluate-router-policy-corpus.mjs --dry-fixture grew from 8 -> 9 cases (added wave25-05-cross-layer: re-pins policy.dry_run_only=true / policy.runtime_replacement=false on the projector, asserts every per_task row's confidence ∈ {high,medium,low} and backend ∈ BACKEND_CLASSES, asserts by_backend totals match the per_task backend union, plus a runtime-replacement policy that the projector surfaces as runtime_replacement=true). Layer B (Rust): handlers::knowledge::plan::tests grew by 2 — router_policy_dry_run_smoke_pins_wave25_invariants pins ALL 8 cross-wave invariants in one shot (mode=off-with-trace-supplied byte-identical to baseline; status=computed proves dry_run_only/runtime_replacement invariants held; applied=Bool(false) literal type-checked; recommended_backend ∈ wave24-01 enum; trace_index_status=used proves wave25-03 path exercised; dispatch fields byte-identical between baseline and dry_run; reasons reference matched rule id; self-audit on plan.rs scans for forbidden std::process::Command / tokio::process / reqwest:: / hyper::Client / openai_api / anthropic_api with strings assembled from parts to avoid self-trip). router_policy_cli_rust_parity_for_high_confidence_match drives the same (5,5) fixture and asserts daemon emits backend=claudecode + confidence=high + status=computed + trace_index_status=used + applied=Bool(false), with inline documentation pinning what the Node CLI's recommend() emits for the SAME shape. Layer C: scripts/check-task-report.mjs --dry-fixture grew from 22 -> 23 (added wave25-05 positive fixture: deterministic-checker backend + high confidence + applied=false + dry_run_only=true with router_reasons + router_trace_index_path filled). cargo test -p missiond-daemon: 1648/0 (was 1646 baseline; +2). cargo build --workspace: clean. git diff --check on 4 staged files: clean. task-scope-guard --mode staged: 4 staged files OK. verify-task-contract: OK against 0f5d857faaa8."
    :commit_hash "0f5d857faaa8"
    :files ["scripts/recommend-task-backend.mjs"
            "scripts/evaluate-router-policy-corpus.mjs"
            "scripts/check-task-report.mjs"
            "crates/missiond-daemon/src/handlers/knowledge/plan.rs"])

  (trace-event
    :id wave25-05-trace-complete-001
    :seq 19
    :at "2026-04-28T17:11:00+08:00"
    :task wave25-05-router-policy-measurement-smoke-v1
    :backend claudecode
    :kind complete
    :summary "wave25-05 complete: cross-layer measurement smoke pins the Wave25 measurable router loop as advisory across all four engines (Node recommendation CLI / Node corpus evaluator / Node report-contract checker / Rust mission_plan daemon). The CLI/Rust parity is now bidirectional — a regression in either engine fails BOTH the Node fixture AND the Rust test for the same (5,5)-event docs-task fixture. All 8 cross-wave invariants from the brief are pinned: policy runtime_replacement=false (1); policy dry_run_only=true (2); applied=false JSON Bool literal in EVERY emitted block (3); renderer's 'advisory' + 'dry-run only' literals (4 — already pinned by wave24-06 daemon smoke + wave25-04 renderer; not re-tested here per write-scope discipline); report-checker rejects router_applied=true / router_dry_run_only=false (5 — pinned by wave25-02 fixtures, untouched by wave25-05); dispatch byte-shape unchanged for mode=off-with-trace-supplied (6); CLI/Rust backend+confidence agreement on (5,5) parity fixture (7); zero shell-out / LLM / git mutation / network in active router code path (8 — Layer B audit scans plan.rs for forbidden Rust patterns assembled from string parts). verify-task-contract: OK against 0f5d857faaa8."))

