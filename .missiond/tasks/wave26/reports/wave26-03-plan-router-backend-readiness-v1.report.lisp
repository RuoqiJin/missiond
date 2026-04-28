;; Wave 26 / Task 03 — mission_plan router backend readiness v1.
;; Schema: missiond.report-contract.v1
;; Source: .missiond/tasks/wave26/wave26-03-plan-router-backend-readiness-v1.lisp

(report wave26-03-plan-router-backend-readiness-v1
  :schema "missiond.report-contract.v1"
  :task_id "wave26-03-plan-router-backend-readiness-v1"
  :status done
  :commit_hash "fcd937a798ba"
  :files_changed
    ["crates/missiond-daemon/src/handlers/knowledge/plan.rs"
     "crates/missiond-mcp/src/tools/knowledge/plan.rs"]

  :acceptance_results
    [(:command "cargo test -p missiond-daemon handlers::knowledge::plan::tests"
      :exit_code 0
      :ok true
      :notes "368/368 (357 prior + 11 new wave26-03 tests). All wave24-04 + wave25-03 + wave25-05 invariants still green: router_policy_mode_off_returns_legacy_response_byte_identical, router_policy_mode_dry_run_does_not_change_dispatch, applied_remains_false_with_trace_index, router_policy_dry_run_smoke_pins_wave25_invariants, etc.")
     (:command "cargo test -p missiond-daemon"
      :exit_code 0
      :ok true
      :notes "1659/1659 — 1648 baseline + 11 new wave26-03 tests. Zero regressions across the full daemon test surface (handlers / state / supervisor / services / e2e).")
     (:command "cargo test -p missiond-mcp --lib"
      :exit_code 0
      :ok true
      :notes "17/17 unchanged from baseline. Adding the new MCP arg to mission_plan's properties bag did not invalidate the existing schema-shape tests (test_directive_plan_workflow_surfaces_registered, test_get_tool, test_handle_tools_list, etc.).")
     (:command "cargo build --workspace"
      :exit_code 0
      :ok true
      :notes "Workspace build OK. 86 pre-existing warnings unchanged (none introduced by wave26-03 — verified by warning count parity vs baseline).")
     (:command "node scripts/check-task-contract.mjs --all"
      :exit_code 0
      :ok true
      :notes "task-contract check OK (83 tasks). All contracts continue to pass shape / scope / must-not-touch / acceptance / commit-policy validation.")
     (:command "git diff --check -- crates/missiond-daemon/src/handlers/knowledge/plan.rs crates/missiond-mcp/src/tools/knowledge/plan.rs"
      :exit_code 0
      :ok true
      :notes "Clean — no whitespace or conflict-marker errors in either edited file.")
     (:command "node scripts/task-scope-guard.mjs --task .missiond/tasks/wave26/wave26-03-plan-router-backend-readiness-v1.lisp --mode staged"
      :exit_code 0
      :ok true
      :notes "task-scope-guard staged OK: wave26-03 (2 staged file(s)), 0 must-not-touch matches.")
     (:command "node scripts/verify-task-contract.mjs .missiond/tasks/wave26/wave26-03-plan-router-backend-readiness-v1.lisp"
      :exit_code 0
      :ok true
      :notes "task-contract verify OK against fcd937a798ba.")
     (:command "node scripts/check-missiond-hooks.mjs --json"
      :exit_code 0
      :ok true
      :notes "Severity ok, reason aligned (.githooks/pre-commit installed and executable).")]

  :time_sinks
    ["read crates/missiond-daemon/src/handlers/knowledge/plan.rs end-to-end at the wave25-03 anchor (lines 9840-10970) to internalise the existing TraceIndexInfo failure-handling pattern + the manual Lisp parser shape (tokenize / TokenCursor / Sexp / parse_router_policy / parse_rule). The wave26-03 registry parser is intentionally a near-mirror of parse_router_policy at the top form level so both consumers share the same tokeniser + cursor."
     "audit the existing test layout (lines 17500..19851) to find fixture_bridge_result / parse_payload / write_temp_docs_policy / write_temp_trace_index helpers and confirm the (5,5)-event high-confidence shape from wave25-05 parity tests. New wave26-03 tests piggyback on write_temp_trace_index to keep the high-confidence prerequisite for the apply-eligible test trivial."
     "design the 6-condition apply-eligibility gate (status=computed + confidence=high + backend in registry + runtime_allowed=true + readiness_status=runtime-ready + apply_blockers empty) to mirror wave26-02's Node helper exactly while preserving the daemon's own enum shape (5 status flavours under backend_registry_status: used | missing | unreadable | malformed | unknown_backend; readiness_status enum extended with 'unknown' for the unknown_backend case). Synthetic blockers carry explicit human-readable text so the rejection reason is grep-able from the JSON without re-running the daemon."
     "verify the off-path early-return in attach_router_recommendation_block by running the byte-identical baseline assertion in router_policy_mode_off_with_registry_and_trace_index_does_no_file_io BEFORE shipping the registry read code path. The Off branch returns before compute_recommendation runs, so no file I/O occurs even when both the wave25-03 and wave26-03 args are populated."]

  :major_decisions
    [(:decision "Registry parser is a purpose-built top-level wrapper around the existing tokenize / TokenCursor / Sexp infrastructure rather than a separate Lisp library."
      :rationale "The wave24-04 manual Lisp parser was already in this module and handles strings / atoms / keywords / lists / brackets / line comments. Forking a second tokeniser would invite drift between the two parsers. parse_backend_registry walks the (router-backend-registry <id> ... (backend ...) ...) shape with the same Sexp enum and reuses sexp_as_text / sexp_as_bool / sexp_as_string_vec helpers (sexp_as_string_vec is new and tolerates Atom/Str/Keyword inside the [...] list).")
     (:decision "router_apply_eligible requires explicit readiness_status == 'runtime-ready', NOT 'current-default'."
      :rationale "Mirrors wave26-02's most load-bearing rule. With the seed registry where claudecode is current-default + runtime_allowed=true + 0 blockers, the gate STILL rejects. This forces any future apply gate to see an explicit runtime-ready opt-in beyond the historical default — preserves the cross-wave invariant that promotion is a deliberate per-backend decision, not an accidental side-effect of being the live default. Documented in three places (the daemon module-level doc-comment, the attach_backend_readiness_fields helper inline, and the blocker string itself: 'backend readiness_status is `current-default`; runtime-ready required'). The wave26-03 test router_policy_mode_dry_run_with_current_default_not_eligible pins this explicitly.")
     (:decision "BackendRegistryInfo carries owned data (Vec<BackendEntry>) rather than borrowed references."
      :rationale "compute_recommendation builds the registry info inside its own scope and threads it through to error_block / rejected_block / computed_block which build the response Value in a final step. Owning the data avoids lifetime gymnastics across three helper functions and matches the wave25-03 TraceIndexInfo pattern (also owned). The registry is small (≤ 5-10 entries in practice) so the Clone cost is negligible.")
     (:decision "Off/default mode does NO file I/O even when BOTH new args are supplied — enforced by the existing attach_router_recommendation_block early-return BEFORE compute_recommendation."
      :rationale "The wave24-04 attach helper already short-circuits when mode==Off. Both load_trace_index (wave25-03) and load_backend_registry (wave26-03) live INSIDE compute_recommendation, so the Off branch never reaches them. The router_policy_mode_off_with_registry_and_trace_index_does_no_file_io test pins this by supplying non-existent paths for BOTH args and asserting byte-identical baseline output. If the daemon attempted to open either file under mode=off the byte-identical assertion would still hold (the recommendation block isn't even spliced) but the read attempt would waste an inode lookup; the early-return prevents even that.")
     (:decision "router_apply_blockers always carries the registry's apply_blockers verbatim plus synthetic gate-failure strings."
      :rationale "Operators must be able to read the rejection reason directly off a single field. Verbatim copy preserves the registry author's intent; appended gate strings (confidence/runtime_allowed/readiness_status/status) make the rejection grep-able from the JSON output without re-running the daemon. The unknown_backend case emits a single sentinel blocker 'recommended_backend `<id>` not in registry' so the simplest failure mode is the first thing operators see.")]

  :unexpected_work
    ["The wave24-04 / wave25-03 attach pattern (error_block / rejected_block / computed_block) needed widening from (policy_source, message, trace) to (policy_source, message, trace, registry, recommended_backend, confidence) so the rejected/error code paths could ALSO surface the readiness fields when a registry is supplied. This let the wave26-03 tests assert backend_registry_status / router_apply_eligible even on rejected policies (the brief did not require this but it falls out naturally from the design and is more correct than a registry-status==absent surface on rejected outcomes). Because rejected/error paths always force confidence=low, the apply-eligibility gate naturally rejects them as well — no special-casing required."
     "Initial draft included an evaluate_apply_eligibility helper function that returned (bool, Vec<String>, Option<&BackendEntry>) but the 'static borrow could not satisfy Rust's lifetime checker; the gate logic was inlined into attach_backend_readiness_fields where it has direct access to the owned BackendEntry inside the Used variant. This eliminates a borrow-vs-clone tradeoff and keeps the gate definition adjacent to where its outputs are spliced — easier to audit when the registry schema grows."]

  :next_actions
    ["Wave26-04 (report contract) can now extend the report schema to surface the new fields once they appear on real plan execute responses; the daemon emits a stable wire shape ready for downstream consumption."
     "Wave26-05 (renderer router readiness context) can read the same six fields off the recommendation block and project them into a markdown render block; the field names match the wave26-02 Node CLI output exactly so a single renderer can serve both surfaces."
     "Wave26-06 (cross-wave smoke) will pin a single canonical fixture under both the daemon (this task) and the wave26-02 Node CLI; the daemon-side parity test in this report (router_policy_mode_dry_run_with_runtime_ready_eligible) is the seed shape for that smoke."])
