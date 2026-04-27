;; Wave 22 / Task 03 — Review LLM approval apply gate v1.
;; Schema: missiond.report-contract.v1
;; Source: .missiond/tasks/wave22/wave22-03-review-llm-approve-apply-gate-v1.lisp

(report wave22-03-review-llm-approve-apply-gate-v1
  :schema "missiond.report-contract.v1"
  :task_id "wave22-03-review-llm-approve-apply-gate-v1"
  :status done
  :commit_hash "4b55cb4"
  :files_changed
    ["crates/missiond-daemon/src/handlers/knowledge/review_gate.rs"
     "crates/missiond-daemon/src/handlers/knowledge/directive.rs"
     "crates/missiond-daemon/src/handlers/knowledge/plan.rs"
     "crates/missiond-mcp/src/tools/knowledge/directive.rs"
     "crates/missiond-mcp/src/tools/knowledge/plan.rs"]

  :acceptance_results
    [(:command "cargo test -p missiond-daemon handlers::knowledge::review_gate::tests"
      :exit_code 0
      :ok true
      :notes "214 passed; 0 failed; 0 ignored. Baseline before this task: 180. +34 new wave22-03 tests covering the apply gate v1 surface end-to-end: compute_proposal_hash determinism + sensitivity (4 tests — same input ⇒ same hash, different action / decision / version perturbations change it, free-text fields ignored); parse_llm_approve_apply_gate_input strict-shape (5 tests — default off, full opt-in, string apply rejected, bool proposal_hash rejected, string caller_approved rejected, null treated as absent); the 6-gate evaluator (G1-G6) plus all 5 wave-21/06 invariants (I1 NeedsChanges skipped non-approved, I2 archive/supersede/remove skipped destructive, I3 proposal block JSON unaffected, I4 unavailable no fallback, I5 deterministic destructive_check overrides model claim); skip-paths (caller_approved=false, confidence medium / low, bundle NoSuggestion, bundle NotInvoked); preflight (missing hash ⇒ APPLY_GATE_MISSING_PROPOSAL_HASH, mismatched ⇒ APPLY_GATE_PROPOSAL_HASH_MISMATCH, case-insensitive match accepts, apply=false skips check, no-proposal+with-hash ⇒ MISMATCH, no-proposal+no-hash ⇒ MISSING); stamp helpers (full block emitted when applied, omitted when not_requested, hash payload stamped when proposal present, skipped when absent).")
     (:command "cargo test -p missiond-daemon handlers::knowledge::directive::tests"
      :exit_code 0
      :ok true
      :notes "43 passed; 0 failed; 0 ignored. No new tests added at this filter (apply gate tests live under handlers::knowledge::review_gate::tests since the gate logic is pure). All wave-15 / wave-18 / wave-21 directive resolution tests stay green — the apply gate threading through action_approve / action_archive (and their _with_resolution / _with_policy_only variants) is byte-additive on the legacy paths because the input parses as default-off when callers omit it.")
     (:command "cargo test -p missiond-daemon handlers::knowledge::plan::tests"
      :exit_code 0
      :ok true
      :notes "278 passed; 0 failed; 0 ignored. Same architectural separation as directive: apply gate logic is in review_gate.rs::tests; plan.rs handler threading is exercised by the existing wave-15 / wave-18 / wave-21 tests which all stay green. action_approve / action_mark / action_supersede now thread the apply gate input but it parses as default-off for legacy callers ⇒ byte-identical with wave-21 / task 06 byte-shape.")
     (:command "cargo test -p missiond-daemon"
      :exit_code 0
      :ok true
      :notes "1496 passed; 0 failed; 0 ignored. Baseline before this task: 1462. +34 new wave22-03 tests; no other tests touched.")
     (:command "cargo test -p missiond-mcp --lib"
      :exit_code 0
      :ok true
      :notes "17 passed; 0 failed; 0 ignored. MCP schema description for `mission_directive` and `mission_plan` extended to advertise the wave22-03 apply gate v1 args (apply_llm_auto_approve, proposal_hash, caller_approved) plus the new `llm_auto_approve_proposal_hash` response field and `llm_approve_apply_gate` block; the test_mission_*_action_enum / test_all_tools_count / test_directive_actions_match_lisp invariants stay green because the action enum and tool count are unchanged.")
     (:command "cargo build --workspace"
      :exit_code 0
      :ok true
      :notes "Build clean. 85 pre-existing warnings unchanged. No new warnings introduced by the wave22-03 changes (the LlmApproveApplyStatus import was unused in directive.rs / plan.rs and removed in a follow-up edit; everything else is consumed by the threaded handler paths).")
     (:command "node scripts/check-architecture-lisp.mjs --all-v2"
      :exit_code 0
      :ok true
      :notes "architecture-lisp check OK (20 files). v2 architecture lisps untouched per :must-not-touch contract; this acceptance gate guards against accidental drift.")
     (:command "git diff --check -- crates/missiond-daemon/src/handlers/knowledge/review_gate.rs crates/missiond-daemon/src/handlers/knowledge/directive.rs crates/missiond-daemon/src/handlers/knowledge/plan.rs crates/missiond-mcp/src/tools/knowledge/directive.rs crates/missiond-mcp/src/tools/knowledge/plan.rs"
      :exit_code 0
      :ok true
      :notes "git diff --check clean across all 5 in-scope files; no whitespace errors introduced by the edits.")]

  :scope_deviations []

  :notes
    "wave22-03 lifts the wave21-06 propose-only LLM auto-approve recommendation into an OPT-IN apply-gate that can drive the actual review transition under strict guards. Hash mismatch / missing fail-fast as structured errors BEFORE any DB mutation per the contract.

review_gate.rs additions (PURE / no I/O — handler reads outcome.status.should_apply() to decide the DB transition):

1. 3 structured-error codes: APPLY_GATE_MISSING_PROPOSAL_HASH, APPLY_GATE_PROPOSAL_HASH_MISMATCH, APPLY_GATE_INVALID_PARAM. Hash codes are returned BEFORE any DB read or mutation per the contract; INVALID_PARAM covers strict-shape rejection of non-bool / non-string knob values.

2. LlmApproveApplyStatus enum: NotRequested | Applied | SkippedUnavailable | SkippedNoProposal | SkippedDestructiveAction | SkippedNonApprovedDecision | SkippedConfidenceTooLow | SkippedCallerNotApproved. Closed wire enum — dashboards can grep for stable strings.

3. ProposalHashStatus enum: NotSupplied | Matches | Mismatch | NoProposalAvailable.

4. LlmApproveApplyGateInput struct + parse_llm_approve_apply_gate_input(args): strict-shape parser. Bool-only for apply / caller_approved (literal string \"true\" rejected so a typo can never silently flip the gate); string-only for proposal_hash. Null treated as absent for back-compat.

5. compute_proposal_hash(action, artifact_id, version, &proposal): SHA-256 over a canonical \"v1|action_lc|artifact|version|decision|confidence|destructive_prefix\" payload, truncated to 32 hex chars. Free-text proposal fields (evidence, non_goal_check) deliberately OUT of the hash so superficial wording differences don't churn the audit correlator. Tested for determinism + sensitivity to action / decision / version, and for invariance under evidence rewording.

6. evaluate_llm_approve_apply_gate(&input, &bundle, action, artifact_id, version): pure 6-gate evaluator. G1 caller opted in (apply=true); G2 proposal_hash matches the bundle's deterministic hash; G3 caller_approved=true (the SECOND opt-in); G4 deterministic non-destructive action (NEVER trusts proposal.destructive_check field — invariant I5); G5 proposal.decision == Approved (invariant I1: never auto-anything-non-approve, never auto-reject); G6 proposal.confidence == High (medium / low SKIP — strictly stricter than wave-21 / task 05). Branches over bundle.status: NotInvoked / Unavailable / NoSuggestion / DestructiveBlocked all collapse to the matching SKIP outcome. Returns LlmApproveApplyGateOutcome { requested, status, applied_decision, proposal_hash_status, computed_proposal_hash, supplied_proposal_hash, caller_approved, safety_rule_results }.

7. enforce_apply_gate_preflight(&input, &bundle, ...): strict pre-flight that returns Err((code, message)) for the two contract-mandated structured errors when apply=true. Skips the check when apply=false. Handler MUST call this BEFORE any DB mutation; on Err the handler returns ToolResult::structured_error and leaves state untouched.

8. stamp_llm_approve_apply_gate_payload(payload, &outcome): inserts the gate block under the stable `llm_approve_apply_gate` key. Skipped when status=NotRequested so legacy callers stay byte-identical with wave-21 / task 06.

9. stamp_proposal_hash_payload(payload, &bundle, action, artifact_id, version): inserts `llm_auto_approve_proposal_hash` (32 hex) under EVERY response that carries a proposal — caller can capture-and-replay it via `proposal_hash` on a follow-up call without re-deriving the hash themselves. Skipped when bundle.proposal is None.

directive.rs handler threading (action=approve and action=archive — archive is destructive and ALWAYS skips with SkippedDestructiveAction per invariant I2):

- action_approve: parses apply_gate_input up-front (strict-shape error ⇒ INVALID_PARAM BEFORE any DB read). When apply=true on the legacy quiet path (no resolution input, no policy promotion), the legacy unconditional directive_approve is INVERTED: handler builds the proposal first (honouring caller-supplied proposer_mode or defaulting to Off so the gate skips with no_proposal), runs the strict pre-flight (hash check ⇒ structured error if mismatch / missing), evaluates the gate, then runs directive_approve ONLY when status.should_apply()==true. On gate skip: NO mutation, status=`llm_auto_apply_skipped`, next_step explains how to retry. On the explicit-resolution path (action_approve_with_resolution): caller-supplied review_decision ALWAYS wins — the gate is INFORMATIONAL ONLY (block stamped, no fail-fast on hash mismatch, no DB mutation driven by gate). Same for action_approve_with_policy_only (deterministic policy already drove the transition).

- action_archive: same parse + thread pattern, but archive is destructive ⇒ gate ALWAYS skips with SkippedDestructiveAction (invariant I2). The block is informational on every archive path; the legacy archive transition is unaffected by apply_llm_auto_approve.

plan.rs handler threading (action=approve, action=mark, action=supersede — supersede is destructive and ALWAYS skips per invariant I2):

- action_approve: same INVERTED legacy quiet path as directive's. Plan version sourced via plan_get(id) BEFORE building the proposal (so the hash is computed against the head). On gate=Applied ⇒ plan_update_status(Approved) + status=approved + resolution_source=llm_approve_apply_gate. On skip ⇒ status=llm_auto_apply_skipped, no mutation. Caller-supplied review_decision still wins on the explicit-resolution path.

- action_mark: gate input threaded through action_mark / action_mark_with_resolution / plan_action_mark_with_policy_only. The legacy mark already runs the requested transition; gate is informational on this path. (A future wave that wants to gate mark-to-approved on the LLM proposal will use the same gate block — no schema change needed.)

- action_supersede: parsed + threaded but supersede is destructive ⇒ gate ALWAYS surfaces SkippedDestructiveAction (I2). Mirrors the wave-21 / task 06 destructive_blocked posture.

5 wave-21 / task 06 invariants — PROVED PRESERVED:

- I1 never auto-reject: G5 only allows decision==Approved. Tests apply_gate_invariant_i1_never_applies_needs_changes pins this. Rejected can never reach the gate (parser already collapsed it to NeedsChanges).
- I2 destructive never auto-promote: G4 deterministic + per-action SkippedDestructiveAction status. Tests apply_gate_invariant_i2_destructive_archive_skipped / supersede_skipped / remove_skipped pin this for the 3 destructive actions.
- I3 propose-only proposal block stays applied=false / requires_human=true: the apply gate publishes its own SEPARATE wire surface (`llm_approve_apply_gate`); the proposal block (`llm_auto_approve_proposal`) is NEVER mutated by the gate. Test apply_gate_invariant_i3_proposal_block_unaffected explicitly asserts proposal.to_json()[\"applied\"]==false BEFORE and AFTER the gate runs (status=Applied path).
- I4 Sonnet unavailable no fallback: gate.status==SkippedUnavailable when bundle.status==Unavailable; never synthesises a deterministic fallback proposal. Test apply_gate_invariant_i4_unavailable_skipped_no_fallback pins this.
- I5 destructive_check ALWAYS deterministic: G4 calls is_destructive_review_action(action) DIRECTLY — never reads proposal.destructive_check as authority. The gate ALSO surfaces a `rule:invariant_i5:deterministic_override` row in safety_rule_results when the model lied (claimed non_destructive for a deterministically-destructive action). Test apply_gate_invariant_i5_destructive_check_always_deterministic constructs a model-lied proposal + destructive action label and asserts the gate trusts the deterministic verdict (status=SkippedDestructiveAction).

Conservative posture: apply_llm_auto_approve defaults to false; the response is byte-identical with wave-21 / task 06 callers when the knob is omitted. Even when opted in, the gate is intentionally STRICTER than wave-21 / task 05 plan-inference apply gate (which allowed medium confidence) because LLM review approval is a higher-stakes surface.

MCP layer surface (crates/missiond-mcp/src/tools/knowledge/directive.rs and plan.rs):
- 3 new schema properties (apply_llm_auto_approve, proposal_hash, caller_approved) with detailed invariant-pinning descriptions.
- The high-level mission_directive / mission_plan tool description strings extended to advertise the wave22-03 contract: 6-gate guard, 5 invariants preserved, hash mismatch / missing fail-fast, destructive ALWAYS skips, caller decision wins on the explicit-resolution path, status=llm_auto_apply_skipped on gate skip.
- The wave-21 / task 06 `auto_approve_mode` description extended to mention `llm_auto_approve_proposal_hash` (the response field callers echo back).

Wave 22 protocol followed:
- Claim entry (wave22-03-claim-001, seq=8) appended to .missiond/tasks/wave22/shared-memory.lisp BEFORE running cargo / staging.
- Edit-only on in-scope files (no Write); Write used only for this report file.
- Pre-commit gate: scripts/task-scope-guard.mjs --mode staged reported 5 staged files (all 5 :write-scope entries), zero touching :must-not-touch (workflow.rs / agent_execution.rs / workstation_dispatch.rs / .missiond/v2/*.lisp / scripts/**).
- MISSIOND_TASK_CONTRACT env var set on the git commit invocation.
- Post-commit verify: scripts/verify-task-contract.mjs cross-checked the commit against the contract.
- Out-of-scope artifacts (this report file + the wave22 shared-memory.lisp claim/completion entries) intentionally NOT staged; both remain untracked / modified-but-unstaged for the next wave22 archive task.

Constraints honored: did not touch crates/missiond-daemon/src/handlers/knowledge/workflow.rs; did not touch crates/missiond-daemon/src/handlers/knowledge/agent_execution.rs; did not touch crates/missiond-daemon/src/handlers/knowledge/workstation_dispatch.rs; did not touch scripts/**; did not touch .missiond/v2/*.lisp. Did not git add . / git stash / git reset / git checkout / git mv / git rm / any other mutating git surface. Used Edit (and Write only for the report) — no Write to in-scope files."
)
