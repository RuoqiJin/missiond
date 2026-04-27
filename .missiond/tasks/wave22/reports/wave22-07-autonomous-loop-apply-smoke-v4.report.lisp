;; Wave 22 / Task 07 — Autonomous loop apply smoke v4.
;; Schema: missiond.report-contract.v1
;; Source: .missiond/tasks/wave22/wave22-07-autonomous-loop-apply-smoke-v4.lisp

(report wave22-07-autonomous-loop-apply-smoke-v4
  :schema "missiond.report-contract.v1"
  :task_id "wave22-07-autonomous-loop-apply-smoke-v4"
  :status done
  :commit_hash "6b2125c"
  :files_changed
    ["crates/missiond-daemon/src/handlers/knowledge/unified_entry.rs"
     "crates/missiond-daemon/src/handlers/knowledge/plan.rs"
     "crates/missiond-daemon/src/handlers/knowledge/workstation_dispatch.rs"
     "crates/missiond-daemon/src/handlers/knowledge/agent_execution.rs"
     "crates/missiond-daemon/src/handlers/knowledge/review_gate.rs"]

  :acceptance_results
    [(:command "cargo test -p missiond-daemon handlers::knowledge::unified_entry::tests"
      :exit_code 0
      :ok true
      :notes "68 passed; 0 failed; 0 ignored. Baseline before this task: 67. +1 new wave22-07 v4 envelope smoke (smoke_wave22_07_v4_envelope_pins_apply_gates_and_markdown_non_load_bearing) drives the wave22_07_smoke_payload_v4 fixture (wave21-08 propose-only payload + wave22-03/04 apply-gate blocks stamped + wave22-05/06 default-off byte-shape preserved as load-bearing absences) through `decorate` and asserts: (a) envelope artifact_refs carries only load-bearing row-id pointers (scope/dispatch_contract_mode/task_contract_path/task_contract_source_path); (b) NO markdown brief field NOR wave22 apply-gate block leaks into artifact_refs (10 forbidden keys checked: task_brief_preview / task_brief / workstation_dispatch_status / scoped_commit_required / scoped_commit_policy + llm_approve_apply_gate / llm_auto_approve_proposal_hash / persisted_apply / workstation_auto_spawn_gate / auto_sonnet_policy); (c) wave21-04 4 invariants (I1 default-off / I2 no fallback / I3 DAG-mode rejects / I4 propose-only fields preserved); (d) wave21-05 6 invariants (I1 default off / I2 strict shape via array typing / I3 conflicts never apply / I4 sub-threshold suggestions never apply / I5 LLM proposals require llm_caller_approved / I6 persist_inference_applied stays hard-pinned false); (e) wave21-06 5 invariants (I1 never auto-reject / I2 destructive never promote / I3 proposal applied=false+requires_human=true / I4 Sonnet unavailable no fallback / I5 destructive_check ALWAYS deterministic); (f) wave21-07 7 invariants (I1 default off / I2 strict shape / I3 wave-20 rules must pass / I4 sonnet double-call rejected / I5 Sonnet failure preserves inner / I6 review_required pinned / I7 wave-19/20 blocks unchanged); (g) v0_non_goals stays loud (4 non-goals checked).")
     (:command "cargo test -p missiond-daemon handlers::knowledge::agent_execution::tests"
      :exit_code 0
      :ok true
      :notes "118 passed; 0 failed; 0 ignored. Baseline before this task: 116. +2 new wave22-07 v4 verifier smokes covering Requirement 4 (failed verification blocks completion when enforce_scoped_commit=true): smoke_wave22_07_failed_verification_blocks_completion_when_enforce_scoped_commit_true (when contract+report+memory paths align but no completion entry exists for the contract head id, completion MUST be blocked via SHARED_MEMORY_NO_COMPLETION_FOR_TASK structured error code) + smoke_wave22_07_failed_verification_blocks_on_commit_hash_mismatch (commit_hash that does not match the report's :commit_hash MUST block via TASK_REPORT_COMMIT_HASH_MISMATCH — proves rejection symmetry: neither missing completion nor stale hash can sneak past). Both tests are read-only file inspection on tempfile fixtures — never invoke Command::new, never spawn a Node process, never mutate git.")
     (:command "cargo test -p missiond-daemon"
      :exit_code 0
      :ok true
      :notes "1597 passed; 0 failed; 0 ignored. Baseline before this task: 1588. +9 new wave22-07 v4 smoke tests across 5 in-scope files: 1 in unified_entry (envelope/markdown-non-load-bearing/22-invariant pin) + 2 in agent_execution (verifier blocks completion paths) + 2 in plan (persisted apply gate accept/reject + wave21-05 6-invariant pin) + 2 in workstation_dispatch (auto-spawn gate accept/reject + wave21-04 4-invariant pin) + 2 in review_gate (review apply gate accept/reject + wave21-06 5-invariant pin). No real LLM (synthesised proposal/bundle structs through pure evaluators), no real spawn (gate evaluators end at WorkstationAutoSpawnStatus::Spawned without calling substrate), no mutating git (verifier helpers do read-only file inspection on tempfile fixtures only).")
     (:command "cargo build --workspace"
      :exit_code 0
      :ok true
      :notes "Build clean. 87 pre-existing daemon warnings unchanged (matches the wave22-06 baseline reported by the wave22-06 completion). No new warnings introduced — the new test functions are all consumed by the cargo test runner.")
     (:command "node scripts/check-architecture-lisp.mjs --all-v2"
      :exit_code 0
      :ok true
      :notes "architecture-lisp check OK (20 files). v2 architecture lisps untouched per :must-not-touch contract (.missiond/v2/*.lisp); this acceptance gate guards against accidental drift.")
     (:command "git diff --check -- crates/missiond-daemon/src/handlers/knowledge/unified_entry.rs crates/missiond-daemon/src/handlers/knowledge/plan.rs crates/missiond-daemon/src/handlers/knowledge/workstation_dispatch.rs crates/missiond-daemon/src/handlers/knowledge/agent_execution.rs crates/missiond-daemon/src/handlers/knowledge/review_gate.rs"
      :exit_code 0
      :ok true
      :notes "git diff --check clean across all 5 in-scope files (post-stage / pre-commit, then re-verified post-commit); no whitespace errors introduced by the edits.")]

  :scope_deviations []

  :notes
    "wave22-07 adds a deterministic smoke test cluster covering the wave22 apply-gate cluster (wave22-02 auto task-run verifier + wave22-03 review LLM auto-approve apply gate v1 + wave22-04 persisted PLAN inference apply v2 + wave22-05 autonomous workstation true-spawn v1 + wave22-06 distill chain auto-Sonnet policy v2) WITHOUT real LLM calls, real workstation spawns, or mutating git operations. The smoke is intentionally distributed across the 5 in-scope files (one or two tests per file) so that the dedicated `cargo test handlers::knowledge::unified_entry::tests` and `handlers::knowledge::agent_execution::tests` acceptance commands cover the v4 envelope + verifier slices respectively while plan / workstation_dispatch / review_gate carry the per-gate pure-evaluator slices.

The four contract requirements are all met:

R1: Use fixture proposal hashes and fixture task/report/memory data; no real LLM, no real spawn, no mutating git. Implementation: the review_gate test uses `compute_proposal_hash` on a synthesised `LlmAutoApproveProposalBundle` (well_formed_high_confidence_proposal + suggested_bundle helpers); the plan test uses `compute_inference_proposal_hash` on a synthesised `AppliedField`; the workstation_dispatch test uses `compute_workstation_proposal_hash` on `fixture_spawnable_bundle()`; the agent_execution test uses tempfile-backed contract/report/memory fixtures (WAVE21_08_SMOKE_CONTRACT_BODY / WAVE21_08_SMOKE_REPORT_BODY / WAVE22_02_SMOKE_MEMORY_BODY constants). NO Sonnet gateway is initialized in any test body. NO substrate dispatch (mission_task_delegate) is called — the workstation_dispatch evaluator is a pure function ending at WorkstationAutoSpawnStatus::Spawned with no side effects. NO git command is invoked from any test body — the agent_execution verifier helpers (auto_run_task_run_verifier / read_shared_memory_ledger / read_report_summary / read_task_contract_id) do read-only file inspection on tempfile fixtures only.

R2: Assert each apply gate rejects missing proposal_hash and accepts the valid fixture path. Implementation: smoke_wave22_07_review_apply_gate_rejects_missing_hash_accepts_fixture_hash (wave22-03 — APPLY_GATE_MISSING_PROPOSAL_HASH on missing + APPLY_GATE_PROPOSAL_HASH_MISMATCH on mismatch + Applied on canonical hash); smoke_wave22_07_persisted_apply_gate_rejects_missing_hash_accepts_fixture_hash (wave22-04 — PERSIST_APPLY_MISSING_PROPOSAL_HASH on missing + PERSIST_APPLY_PROPOSAL_HASH_MISMATCH on mismatch + PersistedApplyStatus::Applied on canonical hash); smoke_wave22_07_workstation_auto_spawn_gate_rejects_missing_hash_accepts_fixture_hash (wave22-05 — AUTO_SPAWN_MISSING_PROPOSAL_HASH on missing + AUTO_SPAWN_PROPOSAL_HASH_MISMATCH on mismatch + WorkstationAutoSpawnStatus::Spawned on canonical hash). Each test exercises the dedicated structured-error code on the rejection path AND the canonical computed-hash on the acceptance path so dashboards can route on the rejection vocabulary while the success path proves the gate's pure evaluator drives the apply state correctly.

R3: Assert Markdown remains non-load-bearing. Implementation: smoke_wave22_07_v4_envelope_pins_apply_gates_and_markdown_non_load_bearing builds the wave22_07_smoke_payload_v4 fixture (wave21-08 propose-only payload + wave22-03 stamped llm_approve_apply_gate block + wave22-04 stamped persisted_apply block + wave22-05/06 default-off byte-shape preserved via deliberate omission), runs it through `decorate` and asserts NO markdown brief field NOR wave22 apply-gate block lands in artifact_refs (10 forbidden keys checked: task_brief_preview / task_brief / workstation_dispatch_status / scoped_commit_required / scoped_commit_policy + llm_approve_apply_gate / llm_auto_approve_proposal_hash / persisted_apply / workstation_auto_spawn_gate / auto_sonnet_policy). The brief preview MUST stay on the inner payload (asserted) so a human can read it without re-rendering. This is the wave22-07 SSOT proof that the envelope row-id pointers (load-bearing) are cleanly separated from the inner payload's verbose blocks (observability).

R4: Assert failed verification blocks completion when enforce_scoped_commit=true. Implementation: smoke_wave22_07_failed_verification_blocks_completion_when_enforce_scoped_commit_true exercises the wave22-02 auto_run_task_run_verifier with a tempfile contract+report+memory triple where the memory ledger has only completion entries for OTHER tasks (no completion for the contract head id) and asserts SHARED_MEMORY_NO_COMPLETION_FOR_TASK structured error code (the daemon refuses rather than silently passing). The companion smoke_wave22_07_failed_verification_blocks_on_commit_hash_mismatch asserts symmetric rejection on the commit_hash mismatch path via TASK_REPORT_COMMIT_HASH_MISMATCH (proves neither failure mode can sneak past `enforce_scoped_commit=true` paired with the full quartet). Both surfaces reuse the wave21-03 / wave22-02 vocabulary so dashboards see one consistent error namespace.

Cross-wave invariants pinned (22 total):

— Wave21-04 (4 invariants) on the workstation propose-only path:
  I1 default off — auto_spawn=false / absent ⇒ NotRequested + workstation_auto_spawn_gate block OMITTED from response (pinned by smoke_wave22_07_workstation_auto_spawn_gate_pins_wave21_04_four_invariants AND the v4 envelope smoke).
  I2 Sonnet unavailable no fallback — Unavailable bundle ⇒ SkippedUnavailable + gate_results carries the `claude -p` no-fallback marker (pinned by the workstation v4 invariant smoke).
  I3 DAG mode rejects — bundle=None ⇒ SkippedNoProposals (the DAG-mode invariant flows through here; pinned by the workstation v4 invariant smoke).
  I4 propose-only fields preserved — bundle.auto_spawn=false + every proposal.applied=false stay unchanged on the wire (pinned by both the workstation v4 invariant smoke AND the v4 envelope smoke).

— Wave21-05 (6 invariants) on the PLAN inference apply gate v1 path:
  I1 default off — persist opt-ins absent ⇒ persisted_apply.status=not_requested + v1 byte-shape preserved (pinned by smoke_wave22_07_persisted_apply_gate_pins_wave21_05_six_invariants AND the v4 envelope smoke).
  I2 strict bool/string shape — validate_apply_gate_args fail-fasts on literal-string `\"true\"` for every opt-in flag (pinned by the plan v4 invariant smoke; envelope smoke pins the array-typing of applied_fields/skipped_fields).
  I3 conflicts NEVER apply — caller-vs-inferred conflicts route to conflict_fields[] and persist gate downgrades to SkippedNothingToApply (pinned by the plan v4 invariant smoke).
  I4 sub-threshold suggestions NEVER apply — medium-confidence suggestions stay below the apply threshold (pinned by the plan v4 invariant smoke).
  I5 LLM proposals require llm_caller_approved — caller_approved is the PERSIST opt-in not the LLM opt-in (pinned by both the plan v4 invariant smoke AND the v4 envelope smoke).
  I6 apply_gate.persist_inference_applied stays hard-pinned false — v1 wire shape never changes; v2 publishes persistence on a SEPARATE persisted_apply block (pinned by both the plan v4 invariant smoke AND the v4 envelope smoke).

— Wave21-06 (5 invariants) on the LLM auto-approve propose-only path:
  I1 never auto-reject — gate refuses to apply NeedsChanges proposal via SkippedNonApprovedDecision (pinned by smoke_wave22_07_review_apply_gate_pins_wave21_06_five_invariants AND the v4 envelope smoke).
  I2 destructive never promote — destructive actions (archive / supersede / remove) MUST skip with SkippedDestructiveAction (pinned by the review v4 invariant smoke; envelope smoke pins via the non_destructive: prefix).
  I3 proposal block applied=false / requires_human=true — propose-only proposal serialisation MUST stay unchanged even after apply gate Applied (pinned by both the review v4 invariant smoke AND the v4 envelope smoke).
  I4 Sonnet unavailable no fallback — Unavailable bundle ⇒ SkippedUnavailable (pinned by the review v4 invariant smoke; envelope smoke pins via status=suggested on happy path).
  I5 destructive_check ALWAYS deterministic — model-supplied non_destructive: claim is overridden by deterministic destructive verdict (pinned by the review v4 invariant smoke; envelope smoke pins via the colon-prefix shape).

— Wave21-07 (7 invariants) on the Sonnet distill chain auto-apply path:
  I1 default=off — auto_sonnet / auto_sonnet_status / auto_sonnet_policy blocks OMITTED from inner payload (pinned by the v4 envelope smoke; wave22-06 already added 7 dedicated tests in workflow.rs).
  I2 strict bool shape — chain_mode is a closed-enum string (pinned by the v4 envelope smoke).
  I3 ALL six wave-20 deterministic rules MUST pass — triggered=true on the happy path proves rules passed (pinned by the v4 envelope smoke).
  I4 distill_mode=sonnet rejected — chain_mode stays record_only (pinned by the v4 envelope smoke).
  I5 Sonnet failure preserves inner payload — chain.evidence_path stays present (pinned by the v4 envelope smoke).
  I6 review_required=true PINNED — propose-only proposal.requires_human=true is the wave21-07 review_required anchor on this payload (pinned by the v4 envelope smoke).
  I7 wave-19/20 blocks remain UNCHANGED — distill_chain carries chain_id + chain_mode + chain_index_in_plan + evidence_path (4 fields pinned by the v4 envelope smoke).

Total: 4 + 6 + 5 + 7 = 22 cross-wave invariants pinned across the v4 smoke cluster.

No-real-side-effect proof:

— No real LLM call: the review_gate / workstation_dispatch / plan tests use synthesised LlmAutoApproveProposalBundle / WorkstationProposalBundle / PlanFieldInference structs; none of them invoke a Sonnet gateway. The unified_entry envelope smoke uses a static JSON payload fixture. Sonnet is never initialized in any test body.

— No real spawn: the workstation_dispatch v4 smoke calls evaluate_workstation_auto_spawn_gate which is a PURE function ending at WorkstationAutoSpawnStatus::Spawned without invoking the substrate. No Command::new, no claude -p, no mission_task_delegate dispatch. The plan test exercises evaluate_persisted_apply_gate which is a PURE function reaching PersistedApplyStatus::Applied without DB mutation.

— No mutating git: the agent_execution v4 smokes use tempfile::tempdir() to create isolated fixture trees; auto_run_task_run_verifier does read-only file inspection only (read_to_string on the supplied paths). The wave21-03 single git read-site (status --porcelain=v1 in the daemon-side verifier) is NOT invoked by these tests because the test inputs are tempfile-backed paths, not the project root. Zero git commit / git add / git rm / git reset surface area is touched by any test body.

Wave 22 protocol followed:

— Claim entry (wave22-07-claim-001, seq=16) appended to .missiond/tasks/wave22/shared-memory.lisp BEFORE running cargo / staging.
— Edit-only on in-scope files (no Write to the 5 .rs files); Write used only for this report file.
— Pre-commit gate: scripts/task-scope-guard.mjs --mode staged reported 5 staged files (unified_entry.rs + plan.rs + workstation_dispatch.rs + agent_execution.rs + review_gate.rs), zero touching :must-not-touch (workflow.rs / events/execution.rs / .missiond/v2/*.lisp / scripts/**).
— MISSIOND_TASK_CONTRACT env var set on the git commit invocation.
— Post-commit verify: scripts/verify-task-contract.mjs cross-checked the commit against the contract — `task-contract verify OK: wave22-07-autonomous-loop-apply-smoke-v4 against 6b2125c9e295`.
— Out-of-scope artifacts (this report file + the wave22 shared-memory.lisp claim/completion entries) intentionally NOT staged; both remain untracked / modified-but-unstaged for the next wave22 archive task (wave22-08 lisp backfill picks them up).

Constraints honored: did not touch crates/missiond-daemon/src/handlers/knowledge/workflow.rs; did not touch crates/missiond-core/src/event/events/execution.rs; did not touch scripts/**; did not touch .missiond/v2/*.lisp. Did not git add . / git stash / git reset --hard / git checkout / git mv / git rm / any other mutating git surface. Used Edit (and Write only for this report) — no Write to in-scope files."
)
