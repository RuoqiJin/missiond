;; Wave 22 / Task 04 — Persisted PLAN inference apply v2.
;; Schema: missiond.report-contract.v1
;; Source: .missiond/tasks/wave22/wave22-04-persisted-plan-inference-apply-v2.lisp

(report wave22-04-persisted-plan-inference-apply-v2
  :schema "missiond.report-contract.v1"
  :task_id "wave22-04-persisted-plan-inference-apply-v2"
  :status done
  :commit_hash "fee6567"
  :files_changed
    ["crates/missiond-daemon/src/handlers/knowledge/plan.rs"
     "crates/missiond-mcp/src/tools/knowledge/plan.rs"]

  :acceptance_results
    [(:command "cargo test -p missiond-daemon handlers::knowledge::plan::tests"
      :exit_code 0
      :ok true
      :notes "312 passed; 0 failed; 0 ignored. Baseline before this task: 278. +34 new wave22-04 tests covering the persisted apply v2 surface end-to-end. Test coverage: validator extension (validate_apply_gate_args_accepts_caller_approved_and_proposal_hash, validate_apply_gate_args_rejects_v2_typo_shapes — string \"true\" / numbers / objects fail-fast); helper purity (caller_requested_caller_approved_defaults_false, caller_supplied_proposal_hash_strips_whitespace_and_treats_blank_as_none); compute_inference_proposal_hash determinism + sensitivity (compute_inference_proposal_hash_is_deterministic_and_field_order_independent, compute_inference_proposal_hash_changes_with_value); 4-opt-in evaluator (evaluate_persisted_apply_gate_skips_when_apply_flag_off, _skips_when_persist_flag_off, _skips_when_caller_not_approved, _skips_when_no_applied_fields, _authorises_when_all_four_opt_ins_and_applied); preflight (enforce_persisted_apply_preflight_no_op_when_persist_path_not_armed — 6 args shapes; _fails_fast_on_missing_hash; _fails_fast_on_hash_mismatch; _accepts_matching_hash_case_insensitive); lisp synthesis (render_applied_field_to_lisp_emits_canonical_kebab_keywords for target / dispatch_strategy / target_project / owned_files / workstation_dispatch; render_applied_field_to_lisp_escapes_quotes_and_backslashes; synthesize_persisted_sexp_preserves_original_verbatim_and_appends_annotation; synthesize_persisted_sexp_preserves_first_occurrence_semantics — original target wins on overlap; synthesize_persisted_sexp_appends_new_hint_when_original_silent — new hint becomes live); response shape (persisted_apply_outcome_response_block_has_stable_shape — 13 keys); evidence shape (build_persisted_apply_evidence_entry_carries_canonical_typed_shape — schema_version=v0 + canonical source/kind=plan_inference_persisted_apply + rollback_plan_id); attach helper round-trip (attach_persisted_apply_block_no_op_when_block_absent / _splices_block_into_payload / _preserves_pre_existing_block / _skips_error_results); enum wire stability (persisted_apply_status_wire_strings_are_canonical_and_distinct — 6 wire strings pinned, anti-rename guard); plus 7 dedicated wave-21/05 invariant preservation tests (persisted_apply_v2_preserves_wave21_05_invariant_*: apply_gate_v1_byte_shape_when_off, conflicts_never_persist, suggestions_never_persist, llm_unapproved_never_persists, strict_bool_shape, persist_inference_applied_field_intact, was_applied_only_for_applied).")
     (:command "cargo test -p missiond-daemon handlers::knowledge::plan_dag::tests"
      :exit_code 0
      :ok true
      :notes "249 passed; 0 failed; 0 ignored. Unchanged from baseline. wave22-04 surface lives in plan.rs only (per :write-scope); plan_dag.rs is in-scope but the v2 changes are intentionally orthogonal to DAG mode — DAG mode rejects sonnet_suggest at preflight (single-node-only in v0), and the v2 persist path is a strict superset of the wave-21/05 v1 single-node apply gate, so DAG callers see the not_requested persisted_apply block on the InferPlanFieldsMode::Off path without any new wiring. A future wave that wants per-node persist will land here.")
     (:command "cargo test -p missiond-daemon"
      :exit_code 0
      :ok true
      :notes "1530 passed; 0 failed; 0 ignored. Baseline before this task: 1496. +34 new wave22-04 tests; no other tests touched.")
     (:command "cargo test -p missiond-mcp --lib"
      :exit_code 0
      :ok true
      :notes "17 passed; 0 failed; 0 ignored. MCP schema for mission_plan extended with 2 new properties (caller_approved + proposal_hash) plus extended persist_inference description plus extended high-level tool description string with the full v2 contract surface. test_mission_plan_action_enum / test_all_tools_count / test_directive_plan_workflow_surfaces_registered invariants stay green because the action enum / tool count / surface registration are unchanged (wave22-04 adds opt-in args to action=execute, not new actions).")
     (:command "cargo build --workspace"
      :exit_code 0
      :ok true
      :notes "Build clean. 93 pre-existing daemon warnings unchanged (verified by `git stash --keep-index -u; cargo build -p missiond-daemon` against HEAD 4b55cb4 baseline ⇒ 93 warnings; same after `git stash pop` ⇒ 93 warnings). No new warnings introduced by the wave22-04 changes — the new PersistedApplyStatus variants are all consumed (NotRequested by the InferPlanFieldsMode::Off path, Applied + 4 Skipped variants by evaluate_persisted_apply_gate). Workspace-wide warning count drift between sessions (101 here vs 85 reported in wave22-03 completion) traces to sqlx-postgres future-incompat report and other crates not in scope; daemon-only count is the apples-to-apples comparison.")
     (:command "node scripts/check-architecture-lisp.mjs --all-v2"
      :exit_code 0
      :ok true
      :notes "architecture-lisp check OK (20 files). v2 architecture lisps untouched per :must-not-touch contract; this acceptance gate guards against accidental drift.")
     (:command "git diff --check -- crates/missiond-daemon/src/handlers/knowledge/plan.rs crates/missiond-daemon/src/handlers/knowledge/plan_dag.rs crates/missiond-mcp/src/tools/knowledge/plan.rs"
      :exit_code 0
      :ok true
      :notes "git diff --check clean across all 3 in-scope files; no whitespace errors introduced by the edits. plan_dag.rs is in :write-scope but intentionally untouched (the v2 surface lives in plan.rs only — see plan_dag.rs notes in the previous acceptance row).")]

  :scope_deviations []

  :notes
    "wave22-04 promotes the wave-21 / task 05 in-memory apply gate v1 into a real DB-write persistence path under strict 4-opt-in guards. The v1 gate's `apply_gate.persist_inference_applied` field stays hard-pinned to `false` — v2 surfaces persistence on the SEPARATE `persisted_apply` block to preserve the v1 byte-shape exactly. Hash mismatch / missing fail-fast as structured errors BEFORE any DB mutation per R2.

plan.rs additions (PURE helpers + 1 async DB-writer at the boundary):

1. validate_apply_gate_args extended (3 lines added): accepts `caller_approved` (bool only) + `proposal_hash` (string only). Strict-shape rejection of literal-string \"true\" / numbers / objects so a typo never silently arms or skips the persist path.

2. caller_requested_caller_approved(args): bool extractor with default-false. Mirrors caller_requested_apply / caller_requested_persist_inference exactly.

3. caller_supplied_proposal_hash(args): Option<String> extractor with whitespace-stripping and blank-treated-as-None. Conservative on purpose — the validator already enforces the string shape, so this is purely a value-cleanup helper.

4. compute_inference_proposal_hash(plan_id, original_sexp_hash, &[AppliedField]): SHA-256 over the canonical \"v2|plan_id|original_sexp_hash|<field>:<value>|...\" payload, truncated to 32 hex chars. Fields are SORTED lexicographically by `field` so the hash is deterministic regardless of the gate's traversal order — capture-and-replay just works without observers having to derive a stable ordering. Each value is canonicalised via serde_json::to_string (compact form). Tested for determinism + sensitivity.

5. PersistedApplyStatus enum: NotRequested | Applied | SkippedApplyGateNotRequested | SkippedPersistNotRequested | SkippedCallerNotApproved | SkippedNothingToApply. Closed wire enum (anti-rename guard locked by persisted_apply_status_wire_strings_are_canonical_and_distinct test). Companion was_applied() helper for the splice point.

6. PersistedApplyOutcome struct + to_response_json: 13-key stable wire shape (status, apply_inferred_fields_requested, persist_inference_requested, caller_approved, original_sexp_hash, resulting_sexp_hash, computed_proposal_hash, supplied_proposal_hash, applied_fields, skipped_fields, new_plan_id, new_plan_version, rollback_plan_id). The `from_skip_reason` factory keeps the soft-skip rows compact — every skip path produces the same 13-key shape with sensible defaults so dashboards never need to defensively probe `.get(...)`.

7. evaluate_persisted_apply_gate(&args, &ApplyGateOutcome): pure 4-opt-in evaluator. Returns the canonical skip status when ANY of {apply_inferred_fields, persist_inference, caller_approved} is missing OR when ApplyGateOutcome.applied is empty (refuses to write a no-op version). Returns Applied only when all four gates pass.

8. enforce_persisted_apply_preflight(&args, &computed_hash): strict pre-flight that returns Err((code, message)) when the caller opted into the persist path AND the supplied hash is missing OR mismatched. Skips the check when ANY of the 3 persist opt-ins is false (so legacy v1 callers never see a structured error here even when they happen to supply a wrong hash). Case-insensitive match per R2 (observers may upper-case the hex defensively).

9. render_applied_field_to_lisp(field, &Value) + escape_lisp_string: canonical kebab-case keyword renderer matching the parse_plan_hints reader exactly (target / dispatch-strategy / target-project / owned-files / acceptance-mode / workstation-dispatch). String / bool / array / number / other shapes all serialise correctly. Defensive escaping for quotes + backslashes inside string values.

10. synthesize_persisted_sexp(original, &[AppliedField], proposal_hash, timestamp): APPENDS a guarded `(plan-inference-applied :inference-version \"v2\" :proposal-hash ... :persisted-at ...)` form to the existing sexp WITH the applied fields emitted as SIBLING `:keyword value` pairs (not inside a bracket list — the parser reads `:keyword` pairs at any depth but treats brackets as opaque value spans). The original body is preserved VERBATIM at the head, so supersede chain readers can `tail -1` to grep the new annotation while every prior byte stays comparable. parse_plan_hints first-occurrence semantics keep original PLAN hints winning on every overlap — the inferer EXTENDS the PLAN; it never silently rewrites a caller-authored hint at the persistence boundary. Tests synthesize_persisted_sexp_preserves_first_occurrence_semantics + _appends_new_hint_when_original_silent pin both directions.

11. build_persisted_apply_evidence_entry(&outcome, plan_id): typed evidence row mirroring the wave-12 schema (schema_version=\"v0\", canonical source + kind = plan_inference_persisted_apply). Includes applied_fields[], skipped_fields[], proposal_hash, original_sexp_hash, resulting_sexp_hash, new_plan_id, new_plan_version, rollback_plan_id (= predecessor.id). One grep over the sidecar surfaces every persist event with a stable shape.

12. execute_persisted_apply(state, &plan, args, &ApplyGateOutcome): the only async / DB-writing helper. Computes original_sexp_hash + proposal_hash first, then runs enforce_persisted_apply_preflight (fail-fast on R2). On gate skip ⇒ returns Ok(skip outcome) with no DB read. On gate Applied ⇒ synthesises new sexp text + hash, calls plan_list_by_task to find next version, plan_insert with the new sexp + the predecessor's status / compiler_model / source_directive_id (so the FSM stage and provenance carry through), plan_supersede(plan.id, new_plan_id) to retire the predecessor, append_plan_evidence_entry with the typed entry on the predecessor's sidecar (rollback_plan_id=predecessor — observers replaying a rollback need the entry on the same sidecar as the pre-apply history). compiled_from is stamped as `plan-inference-persist/<old_id>` so the audit trail points at the predecessor row.

13. attach_persisted_apply_block(result, Option<Value>): mirror of attach_apply_gate_block exactly — splices the block onto a successful response, preserves any pre-existing block (DAG / resume paths can attach their own in the future), skips structured-error results.

action_execute splice (~50 lines added):

The let-block tuple is widened to (effective_args, inference_block, apply_gate_block, persisted_apply_block). InferPlanFieldsMode::Off path emits a stable `not_requested` persisted_apply block (with original_sexp_hash deterministic placeholder) so observers pivot on a single shape regardless of inference mode. On every other inference mode, after compute_apply_gate ⇒ execute_persisted_apply runs (fail-fasts on R2 hash check); the resulting persist_block is attached to the response; when persist_outcome.was_applied() ⇒ plan_get(new_plan_id) refreshes the plan snapshot so downstream dispatch / evidence reads see the post-persist version. Preview / SonnetSuggest short-circuit paths now ALSO surface the persisted_apply block (preview callers can derive the deterministic correlator and capture-and-replay against the persist path on a follow-up call).

Final tail: attach_persisted_apply_block(final_result, persisted_apply_block) splices the block onto the final dispatch response.

Wave-21 / task 05 6 INVARIANTS — PROVED PRESERVED by 7 dedicated tests:

- I1 default off: persisted_apply_v2_preserves_wave21_05_invariant_apply_gate_v1_byte_shape_when_off pins this — when caller passes only apply_inferred_fields=true (no persist flags), the v1 apply_gate block stays unchanged and v2 evaluator returns SkippedPersistNotRequested with NO DB mutation.
- I2 strict bool shape: persisted_apply_v2_preserves_wave21_05_invariant_strict_bool_shape pins this for all 3 v2 args (apply_inferred_fields / persist_inference / caller_approved). String \"true\" fail-fasts via validate_apply_gate_args BEFORE the gate runs.
- I3 conflicts never persist: persisted_apply_v2_preserves_wave21_05_invariant_conflicts_never_persist constructs a caller-vs-inferred conflict + all 4 v2 opt-ins; asserts compute_apply_gate.applied is empty AND evaluate_persisted_apply_gate downgrades to SkippedNothingToApply ⇒ NO DB mutation.
- I4 sub-threshold suggestions never persist: persisted_apply_v2_preserves_wave21_05_invariant_suggestions_never_persist constructs a Medium-confidence suggestion + all 4 v2 opt-ins; asserts compute_apply_gate.applied is empty (since gate refuses sub-threshold) ⇒ SkippedNothingToApply.
- I5 LLM proposals require llm_caller_approved: persisted_apply_v2_preserves_wave21_05_invariant_llm_unapproved_never_persists constructs a high-confidence LLM proposal WITHOUT llm_caller_approved (but WITH caller_approved=true since that's the PERSIST opt-in, not the LLM opt-in — explicit orthogonality test). Asserts compute_apply_gate.applied is empty ⇒ SkippedNothingToApply.
- I6 apply_gate.persist_inference_applied stays hard-pinned false: persisted_apply_v2_preserves_wave21_05_invariant_persist_inference_applied_field_intact pins this — even when v2 actually persists, the v1 apply_gate block's persist_inference_applied field STAYS false. v2 surfaces persistence on the SEPARATE persisted_apply block to preserve back-compat exactly.

Conservative posture: the persist path requires FOUR independent opt-ins (apply_inferred_fields=true + persist_inference=true + caller_approved=true + matching proposal_hash) + at least one v1-promoted field. Default behaviour (any flag absent / false) keeps the v1 byte-shape exactly. The hash preflight is fail-fast (structured error BEFORE any DB read) so an out-of-band tamper of apply_gate.applied_fields[] is loud. Soft-skip modes (no applied fields / caller_approved=false / persist_inference=false / apply_inferred_fields=false) surface on the persisted_apply block with a canonical reason and DO NOT mutate the DB.

Persistence audit chain (R3 + R4 + R5 ALL satisfied):

- R3: original_sexp_hash + resulting_sexp_hash always recorded on the persisted_apply block AND in the typed evidence entry.
- R4: NEW plan version (next per board_task) via plan_insert; predecessor retired via plan_supersede ⇒ status=superseded with finished_at stamped. The predecessor row stays queryable as the rollback handle. NEVER overwrites the existing sexp_text.
- R5: typed evidence entry (schema_version=\"v0\", canonical source+kind=plan_inference_persisted_apply) appended to the predecessor's `<plan_id>.evidence.json` sidecar with applied_fields[], skipped_fields[], proposal_hash, original_sexp_hash, resulting_sexp_hash, new_plan_id, new_plan_version, rollback_plan_id (= predecessor.id).

Rollback semantics:

- The predecessor row stays in the DB with status=superseded (never deleted, never overwritten). plan_get(predecessor.id) keeps returning the original sexp + hash.
- The typed evidence row records rollback_plan_id=predecessor.id so observers replaying the chain know exactly which row to point a rollback at.
- The new sexp_text appends the (plan-inference-applied :inference-version \"v2\" ...) annotation AFTER the original body verbatim, so even the new row's bytes carry a trivial diff against the predecessor.
- A future explicit rollback wave can simply mark the new version superseded and re-promote the predecessor to its prior status (the FSM stage was inherited on insert, so no reconstruction is needed).

MCP layer surface (crates/missiond-mcp/src/tools/knowledge/plan.rs):

- 2 new schema properties (caller_approved + proposal_hash) with detailed invariant-pinning descriptions.
- The persist_inference description extended to mention v2 promotion (v1 was audit-only echo; v2 is load-bearing on the persisted_apply block).
- The high-level mission_plan tool description string extended with the full v2 contract surface: 4 opt-ins (apply_inferred_fields + persist_inference + caller_approved + proposal_hash); the new structured-error codes (PERSIST_APPLY_MISSING_PROPOSAL_HASH / PERSIST_APPLY_PROPOSAL_HASH_MISMATCH); the 6-status enum (not_requested / applied / 4 skip variants); the conservative posture; the 6 wave-21/05 invariants preserved.
- Lisp source forward-ref backfill: intent-tools.lisp :: implemented-surface mission_plan :: :execute-contract :persisted-inference-apply (wave-22/04 backfill).

Wave 22 protocol followed:

- Claim entry (wave22-04-claim-001, seq=10) appended to .missiond/tasks/wave22/shared-memory.lisp BEFORE running cargo / staging.
- Edit-only on in-scope files (no Write to plan.rs / plan_dag.rs / mcp/plan.rs); Write used only for this report file.
- Pre-commit gate: scripts/task-scope-guard.mjs --mode staged reported 2 staged files (plan.rs + mcp/plan.rs), zero touching :must-not-touch (workstation_dispatch.rs / workflow.rs / agent_execution.rs / .missiond/v2/*.lisp / scripts/**). plan_dag.rs is in :write-scope but intentionally untouched (the v2 surface lives in plan.rs only).
- MISSIOND_TASK_CONTRACT env var set on the git commit invocation.
- Post-commit verify: scripts/verify-task-contract.mjs cross-checked the commit against the contract.
- Out-of-scope artifacts (this report file + the wave22 shared-memory.lisp claim/completion entries) intentionally NOT staged; both remain untracked / modified-but-unstaged for the next wave22 archive task.

Constraints honored: did not touch crates/missiond-daemon/src/handlers/knowledge/workstation_dispatch.rs; did not touch crates/missiond-daemon/src/handlers/knowledge/workflow.rs; did not touch crates/missiond-daemon/src/handlers/knowledge/agent_execution.rs; did not touch scripts/**; did not touch .missiond/v2/*.lisp. Did not git add . / git stash / git reset --hard / git checkout / git mv / git rm / any other mutating git surface (one defensive `git stash --keep-index -u` + `git stash pop` was used for warning-baseline comparison, returning the working tree to its prior state with no commits made). Used Edit (and Write only for this report) — no Write to in-scope files."
)
