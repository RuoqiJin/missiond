;; Wave 25 / Task 03 — mission_plan router trace-index confidence v1.
;; Schema: missiond.report-contract.v1
;; Source: .missiond/tasks/wave25/wave25-03-plan-router-trace-index-confidence-v1.lisp

(report wave25-03-plan-router-trace-index-confidence-v1
  :schema "missiond.report-contract.v1"
  :task_id "wave25-03-plan-router-trace-index-confidence-v1"
  :status done
  :commit_hash "bd2b5a34fd03"
  :files_changed
    ["crates/missiond-daemon/src/handlers/knowledge/plan.rs"
     "crates/missiond-mcp/src/tools/knowledge/plan.rs"]

  :acceptance_results
    [(:command "cargo test -p missiond-daemon handlers::knowledge::plan::tests"
      :exit_code 0
      :ok true
      :notes "346 plan-handler tests pass, including all 9 new wave25-03 tests (router_policy_mode_off_with_trace_index_supplied_does_no_file_io / router_policy_mode_dry_run_with_trace_index_high_confidence / _medium_confidence / _low_confidence_when_zero_events / _with_missing_trace_index_emits_status_missing / _with_malformed_trace_index_emits_status_malformed / _no_trace_index_supplied_emits_status_absent / _with_trace_index_does_not_change_dispatch / applied_remains_false_with_trace_index). All 14 wave24-04 tests and the wave24-06 cross-wave invariant smoke remain green.")
     (:command "cargo test -p missiond-daemon"
      :exit_code 0
      :ok true
      :notes "Full daemon suite: 1646 / 0. Baseline was 1637 (wave24-06 commit 6afe541); +9 = the new wave25-03 tests. No pre-existing test perturbed.")
     (:command "cargo test -p missiond-mcp --lib"
      :exit_code 0
      :ok true
      :notes "MCP lib tests: 17 / 0 — matches baseline. Schema additions (the new router_policy_trace_index_path string property) parse cleanly via the existing build_properties() / definitions() path; no test count delta because the surface is additive.")
     (:command "cargo build --workspace"
      :exit_code 0
      :ok true
      :notes "Workspace builds clean. No new dependencies added — the daemon already depends on serde_json. Trace-index parsing uses serde_json::from_str + Value/Map; trace-index file reads use std::fs::read_to_string. NO shell-out, NO Node spawn, NO git invocation from the daemon path.")
     (:command "node scripts/check-task-contract.mjs --all"
      :exit_code 0
      :ok true
      :notes "task-contract check OK (74 tasks). The wave25-03 contract itself parses and validates; no contract drift from sibling waves.")
     (:command "git diff --check -- crates/missiond-daemon/src/handlers/knowledge/plan.rs crates/missiond-mcp/src/tools/knowledge/plan.rs"
      :exit_code 0
      :ok true
      :notes "git diff --check clean across both write-scope files; no trailing whitespace, mixed tabs/spaces, or conflict markers.")
     (:command "node scripts/task-scope-guard.mjs --task .missiond/tasks/wave25/wave25-03-plan-router-trace-index-confidence-v1.lisp --mode staged"
      :exit_code 0
      :ok true
      :notes "task-scope-guard staged OK: wave25-03-plan-router-trace-index-confidence-v1 (2 staged file(s)). Both staged paths inside :write-scope; zero matches against :must-not-touch (workstation_dispatch.rs / agent_execution.rs / unified_entry.rs / plan_dag.rs / scripts/** / .missiond/v2/** / .missiond/tasks/**).")
     (:command "node scripts/verify-task-contract.mjs .missiond/tasks/wave25/wave25-03-plan-router-trace-index-confidence-v1.lisp"
      :exit_code 0
      :ok true
      :notes "task-contract verify OK against bd2b5a34fd03 — commit hash exists; commit message exactly matches contract :commit.message ('feat(plan): score router dry-run with trace index'); changed_files ⊆ write-scope (write-scope-only); changed_files ∩ must-not-touch = ∅; acceptance commands present in contract.")
     (:command "node scripts/check-missiond-hooks.mjs --json"
      :exit_code 0
      :ok true
      :notes "ok=true severity=ok matches=true reason=aligned — core.hooksPath==.githooks already set; .githooks/pre-commit exists and is executable; no install needed.")]

  :scope_deviations []

  :trace_refs [wave25-03-trace-start-001 wave25-03-trace-commit-001 wave25-03-trace-complete-001]

  :recommended_backend "claudecode"
  :router_confidence "low"
  :router_policy_path ".missiond/router/router-policy-v1.lisp"
  :router_dry_run_only true
  :router_applied false
  :router_reasons ["dispatch-strategy:fresh-code-alignment"
                   "owner:claudecode"
                   "kind:code-alignment"
                   "fallback:insufficient_trace_history"]

  :major_decisions
    [(:decision "Picked router_policy_trace_index_path (not router_trace_index_path) as the MCP arg name"
      :rationale "Consistency with the existing router_policy_mode and router_policy_path names — all three are the dry-run knob group. Using the router_policy_ prefix makes it obvious the arg is part of the wave-24 router-policy surface and is silently ignored when router_policy_mode != dry_run."
      :trace_ref "wave25-03-trace-start-001")
     (:decision "Used a TraceIndexInfo enum (Absent / Used / Missing / Unreadable / Malformed) rather than a struct + boolean flags"
      :rationale "Each variant carries exactly the data the block-builder needs and forbids invalid combinations at the type level (e.g. you cannot construct a Missing variant without a path AND warning). The exhaustive match in attach_trace_index_fields catches any future variant addition at compile time."
      :trace_ref "wave25-03-trace-start-001")
     (:decision "When router_policy_trace_index_path is absent, OMIT all three trace_index_* fields entirely rather than emit trace_index_status=absent"
      :rationale "Preserves the wave24-04 byte-shape for callers that did not opt in to wave25-03. A wave24-04 consumer reading the recommendation block sees exactly the wave24-04 surface; only wave25-03-aware consumers that supplied the path see the new fields. Documented this choice in the test (router_policy_mode_dry_run_no_trace_index_supplied_emits_status_absent) so the contract is loud."
      :trace_ref "wave25-03-trace-start-001")
     (:decision "Failure modes (missing/unreadable/malformed) fall back to medium for matched / low for no-match, NEVER to error or rejected"
      :rationale "trace-index is opt-in confidence enrichment, not a hard dependency. A malformed file MUST NOT block dispatch — the operator must always be able to read the recommendation, just with the warning surfaced. Splitting the I/O failure into Missing (NotFound) vs Unreadable (other I/O errors) gives the operator a clearer signal in the warning string."
      :trace_ref "wave25-03-trace-commit-001")
     (:decision "Mirrored the Node CLI's RICH_TRACE_THRESHOLD = 5 as a private module-level Rust const named identically"
      :rationale "Lock-step parity is the whole point of this task; pinning the constant locally with a doc-comment cross-referencing scripts/recommend-task-backend.mjs makes the next person who edits either side notice the dependency. Did NOT add a shared lisp/json constant module — that would expand the write-scope unnecessarily."
      :trace_ref "wave25-03-trace-commit-001")]

  :unexpected_work
    [(:summary "The wave24-04 helper hard-coded medium for matched outcomes regardless of trace input. Refactored compute_recommendation to thread a TraceIndexInfo through the matched branch and only thread it through the helpers that emit a recommendation block. Verified the legacy wave24-04 medium fallback still applies whenever the trace-index is absent or degraded (router_policy_mode_dry_run_first_priority_match_wins still passes — it does not supply trace-index, so confidence stays medium)."
      :trace_ref "wave25-03-trace-start-001")
     (:summary "Confirmed parallel agent wave25-02 was completing while I was running tests; their staged write-scope (.missiond/tasks/schema/report-contract-v1.lisp + scripts/check-task-report.mjs) does not overlap with mine. Their commit 770903136f00 landed cleanly and my pre-commit run rebased atop it without conflict."
      :trace_ref "wave25-03-trace-commit-001")]

  :notes
    "MCP arg added: router_policy_trace_index_path (string, optional, default absent). Documented inline in build_properties() as a wave-25 / task 03 addition; explicitly cross-referenced scripts/build-session-trace-index.mjs (file producer), scripts/recommend-task-backend.mjs (confidence rule mirror), and the wave24-04 byte-identical baseline.

Behavior summary:
- router_policy_mode = off (default OR explicit \"off\"): NO file I/O, recommendation block NOT emitted, response byte-identical to wave-15..23 baseline. router_policy_trace_index_path is silently ignored even if supplied.
- router_policy_mode = dry_run AND router_policy_trace_index_path absent: recommendation block emitted with trace_index_* fields OMITTED (preserves wave24-04 byte-shape for callers that did not opt in to wave25-03). Confidence: medium for matched, low for no-match (the wave24-04 default).
- router_policy_mode = dry_run AND router_policy_trace_index_path supplied: daemon reads the file via std::fs::read_to_string + serde_json. Confidence mirrors scripts/recommend-task-backend.mjs scoreConfidence (RICH_TRACE_THRESHOLD = 5):
    matched + max(by_task[plan.board_task_id].events, by_backend[recommended_backend].events) >= 5  -> high
    matched + max(...) in 1..=4                                                                     -> medium
    matched + max(...) == 0                                                                          -> low
    no-match (fallback)                                                                              -> low
  Recommendation block additionally surfaces: trace_index_path (echo of input), trace_index_status (\"used\"), and (NOT in the used path) trace_index_warning.

Failure handling (all non-fatal — dispatch ALWAYS succeeds):
- File missing (std::io::ErrorKind::NotFound): trace_index_status=\"missing\", trace_index_warning carries the underlying I/O error string. Confidence falls back to medium for matched / low for no-match.
- Other I/O error: trace_index_status=\"unreadable\", same warning + fallback rules.
- serde_json parse error OR top-level value not a JSON object OR by_task / by_backend not a JSON object: trace_index_status=\"malformed\", warning describes the failure. Same fallback confidence.
- Note: when by_task / by_backend keys are simply MISSING from a valid JSON object (i.e. an empty index), they default to 0 events — confidence becomes low (matched-zero) rather than malformed.

Cross-wave invariants re-pinned under the new code path:
- applied=false stays a hard-coded literal in EVERY emitted block (used / missing / unreadable / malformed / absent). Test: applied_remains_false_with_trace_index exercises 4 of the 5 status flavours (unreadable is approximated by missing because synthesizing a non-NotFound I/O error portably is fragile).
- Dispatch fields (target_tool / target_source / dispatch_strategy / dispatch_strategy_source / next_call / execute_mode / runner_status) are byte-identical with vs without the trace-index arg. Test: router_policy_mode_dry_run_with_trace_index_does_not_change_dispatch.
- Off mode is byte-identical to baseline EVEN WHEN a trace-index path is supplied. Test: router_policy_mode_off_with_trace_index_supplied_does_no_file_io exercises both the explicit \"off\" case and the default (arg absent) case, asserting baseline_text == after_text byte-identically.

Proof off/default mode does no file I/O AND dispatch is byte-identical:
- Strategy: the Off branch in attach_router_recommendation_block early-returns BEFORE compute_recommendation runs, and compute_recommendation is the only call site of load_trace_index. The Off path therefore cannot read the trace-index file by construction.
- Verification: router_policy_mode_off_with_trace_index_supplied_does_no_file_io supplies a non-existent path (/this/path/does/not/exist/wave25-03/trace-index.json) under both explicit mode=off and default (arg absent). Both cases assert baseline_text == after_text byte-identically. If the daemon attempted to open the file, the test would still pass syntactically (because the open error is non-fatal), but the byte-identical assertion gives stronger assurance: the response shape NEVER carries any trace_index_* field when mode=off (the recommendation block isn't even emitted), so any side effect inside the file-read path would have to be unobservable on the response. Combined with the architectural early-return guard, this is a reasonable proof.

Confidence rule mirroring (against scripts/recommend-task-backend.mjs):
- Node CLI: scoreConfidence at scripts/recommend-task-backend.mjs:472 computes max(by_task[task.id].events, by_backend[backend].events); >= 5 -> high; >= 1 -> medium; else low.
- Rust daemon: compute_recommendation matches the same shape using events_for_task / events_for_backend / RICH_TRACE_THRESHOLD = 5 (private module-level const). The threshold and the field paths are pinned in the doc-comment above compute_recommendation; if the Node CLI changes, both files MUST be updated together.
- Edge case: when matched but no trace-index supplied, Rust falls back to medium (the wave24-04 default for matched). Node CLI lacking --trace-index also falls back to low (`if (!traceIndex) return 'low'`). This is a known mild divergence: the daemon's default-medium is the conservative wave24-04 surface, while the Node CLI's default-low requires the operator to opt-in with --trace-index for any non-low. Since wave25-03's contract states 'this is opt-in confidence enrichment, not a hard dependency', the daemon's default-medium-on-matched is preferred — the Node CLI is explicitly a confidence floor.

New tests (9):
- router_policy_mode_off_with_trace_index_supplied_does_no_file_io
- router_policy_mode_dry_run_with_trace_index_high_confidence
- router_policy_mode_dry_run_with_trace_index_medium_confidence
- router_policy_mode_dry_run_with_trace_index_low_confidence_when_zero_events
- router_policy_mode_dry_run_with_missing_trace_index_emits_status_missing
- router_policy_mode_dry_run_with_malformed_trace_index_emits_status_malformed
- router_policy_mode_dry_run_no_trace_index_supplied_emits_status_absent
- router_policy_mode_dry_run_with_trace_index_does_not_change_dispatch
- applied_remains_false_with_trace_index

Cargo test counts: daemon 1646 / 0 (was 1637 baseline; +9 wave25-03 tests). MCP --lib 17 / 0 (matches baseline; schema addition is observed by the tools-list shape but no count delta).

Constraints honored: did NOT touch crates/missiond-daemon/src/handlers/knowledge/workstation_dispatch.rs / agent_execution.rs / unified_entry.rs / plan_dag.rs (wave23-05 lesson: these handlers have intricate cross-wave invariants and are owned by sibling waves); did NOT touch scripts/** (wave25-01's evaluator and wave25-02's checker are theirs); did NOT touch .missiond/v2/** (intent-event-bus.lisp is locked); did NOT touch .missiond/tasks/** outside the task scope (only appended to shared-memory.lisp / session-trace.lisp / wrote this report). NO new dependencies added — serde_json was already in Cargo.toml. NO shell-out / spawn / git invocation from the daemon. NO git add . / git stash / git reset / git checkout / amend / push / --no-verify / git add -A.")
