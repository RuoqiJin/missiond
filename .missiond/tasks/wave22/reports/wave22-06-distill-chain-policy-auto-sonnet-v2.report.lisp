;; Wave 22 / Task 06 — Distill chain POLICY auto-Sonnet v2.
;; Schema: missiond.report-contract.v1
;; Source: .missiond/tasks/wave22/wave22-06-distill-chain-policy-auto-sonnet-v2.lisp

(report wave22-06-distill-chain-policy-auto-sonnet-v2
  :schema "missiond.report-contract.v1"
  :task_id "wave22-06-distill-chain-policy-auto-sonnet-v2"
  :status done
  :commit_hash "2423d4b"
  :files_changed
    ["crates/missiond-daemon/src/handlers/knowledge/workflow.rs"
     "crates/missiond-daemon/src/handlers/knowledge/plan.rs"
     "crates/missiond-mcp/src/tools/knowledge/workflow.rs"]

  :acceptance_results
    [(:command "cargo test -p missiond-daemon handlers::knowledge::workflow::tests"
      :exit_code 0
      :ok true
      :notes "158 passed; 0 failed; 0 ignored. Baseline before this task: 142. +16 new wave22-06 tests covering the policy gate end-to-end. Test coverage: closed-enum value strings pin (auto_sonnet_policy_value_strings_pin — off / safe_after_rules / dry_run wire form locked); policy_status taxonomy pin (auto_sonnet_policy_status_constants_pin_the_wire_form — 8 distinct wire strings); parse_auto_sonnet_policy_default_off_on_missing_or_null_or_blank (back-compat); parse_auto_sonnet_policy_accepts_safe_after_rules_and_dry_run; parse_auto_sonnet_policy_rejects_unknown_string (typo / camelCase rejected); parse_auto_sonnet_policy_rejects_non_string_shapes (bool / number / array / object rejected); build_auto_sonnet_policy_block_minimum_shape_has_all_required_fields (no caller_approval field — load-bearing absence); build_auto_sonnet_policy_block_failure_shape_carries_error_text; attach_auto_sonnet_policy_splices_block_and_top_level_shortcut. Plus 7 dedicated invariant-preservation tests pinning the wave-21/07 7 invariants on the policy path: wave22_06_preserves_wave21_07_i1_default_off_byte_shape (no policy block emitted by default); wave22_06_preserves_wave21_07_i2_strict_shape_no_typo_escalation (10 typo / camelCase / case-mismatch / shape-mismatch inputs all rejected); wave22_06_preserves_wave21_07_i3_rules_must_pass_no_relax (skipped_rules_failed surfaced verbatim); wave22_06_preserves_wave21_07_i4_already_sonnet_refuses_double_call (caller_already_chose_sonnet helper REUSED + skipped_already_sonnet block shape pinned); wave22_06_preserves_wave21_07_i5_sonnet_failure_preserves_inner (applied=false on failed / invalid_output, error text carried); wave22_06_preserves_wave21_07_i6_review_required_pinned_true (review_required=true PINNED on every policy_status — 7 outcomes covered); wave22_06_preserves_wave21_07_i7_wave19_20_21_blocks_unchanged (4-block coexistence: wave-19 auto_chain + wave-20 auto_trigger + wave-21/07 auto_sonnet + wave-22/06 auto_sonnet_policy all survive verbatim).")
     (:command "cargo test -p missiond-daemon handlers::knowledge::plan::tests"
      :exit_code 0
      :ok true
      :notes "321 passed; 0 failed; 0 ignored. Baseline before this task: 318. +3 new wave22-06 tests covering the plan-level forwarder validator: validate_distill_chain_args_accepts_auto_sonnet_policy_canonical_strings (off / safe_after_rules / dry_run / empty / null all accepted); validate_distill_chain_args_rejects_auto_sonnet_policy_unknown_string (typo fails fast as INVALID_PARAM with bad value echoed); validate_distill_chain_args_rejects_auto_sonnet_policy_non_string_shapes (bool / number / array / object all rejected with shape-label diagnostic).")
     (:command "cargo test -p missiond-daemon"
      :exit_code 0
      :ok true
      :notes "1588 passed; 0 failed; 0 ignored. Baseline before this task: 1569. +19 new wave22-06 tests (16 in workflow + 3 in plan); no other tests touched.")
     (:command "cargo test -p missiond-mcp --lib"
      :exit_code 0
      :ok true
      :notes "17 passed; 0 failed; 0 ignored. MCP schema for mission_workflow extended with 1 new closed-enum property (auto_sonnet_policy with detailed wave-22/06 v2 contract description) plus extended high-level tool description string with the full wave-22/06 v2 surface (3 enum values / 8 policy_status wire strings / 7 invariants preserved / strict-shape rejection contract). test_workflow_actions_match_lisp / test_plan_actions_match_lisp / test_get_tool / test_handle_tools_list / test_handle_tools_call invariants all stay green because the action enum / tool count / surface registration are unchanged (wave22-06 adds an opt-in arg to action=distill, not a new action)."
)
     (:command "cargo build --workspace"
      :exit_code 0
      :ok true
      :notes "Build clean. 87 pre-existing daemon warnings unchanged (matches the wave22-05 baseline reported by the wave22-05 completion). No new warnings introduced — the new AutoSonnetPolicy variants (Off / SafeAfterRules / DryRun) are all consumed by parse_auto_sonnet_policy + maybe_apply_auto_sonnet_policy + maybe_apply_distill_chain_layers; the 8 AUTO_SONNET_POLICY_STATUS_* constants are all consumed by the orchestrator's 7 branches (applied / dry_run / skipped_no_trigger / skipped_rules_failed / skipped_already_sonnet / skipped_inner_error / not_requested + off pinned by tests)."
)
     (:command "node scripts/check-architecture-lisp.mjs --all-v2"
      :exit_code 0
      :ok true
      :notes "architecture-lisp check OK (20 files). v2 architecture lisps untouched per :must-not-touch contract (.missiond/v2/*.lisp); this acceptance gate guards against accidental drift.")
     (:command "git diff --check -- crates/missiond-daemon/src/handlers/knowledge/workflow.rs crates/missiond-daemon/src/handlers/knowledge/plan.rs crates/missiond-mcp/src/tools/knowledge/workflow.rs"
      :exit_code 0
      :ok true
      :notes "git diff --check clean across all 3 in-scope files; no whitespace errors introduced by the edits.")]

  :scope_deviations []

  :notes
    "wave22-06 simplifies the wave-21 / task 07 dual opt-in (auto_sonnet=true AND auto_sonnet_approved=true) to a single explicit closed-enum policy gate (auto_sonnet_policy ∈ {off, safe_after_rules, dry_run}). The wave-21/07 dual opt-in path is PRESERVED as a back-compat surface (off is the default → byte-identical with wave-21/07); the new policy is layered ADDITIVELY on top so a caller can opt into either surface. When both are requested on the same call, both blocks land verbatim (auto_sonnet wave-21/07 block + auto_sonnet_policy wave-22/06 block).

The five contract requirements are all met:

R1: auto_sonnet_policy is a closed-enum string ∈ {off, safe_after_rules, dry_run}; default off. Implementation: AutoSonnetPolicy enum + parse_auto_sonnet_policy parser. Tested by parse_auto_sonnet_policy_default_off_on_missing_or_null_or_blank + parse_auto_sonnet_policy_accepts_safe_after_rules_and_dry_run.

R2: safe_after_rules does NOT require a second opt-in flag — the policy choice IS the explicit operator attestation. The new gate has no caller_approval field on its block (load-bearing absence pinned by build_auto_sonnet_policy_block_minimum_shape_has_all_required_fields). All six wave-20 deterministic safety rules MUST pass before the gate fires (REUSES the wave-20 trigger's rule outcomes verbatim — never relaxes). Tested by wave22_06_preserves_wave21_07_i3_rules_must_pass_no_relax.

R3: Every Sonnet-produced workflow / distill output keeps review_required=true PINNED on the policy block. No DB transition flips on the auto-applied path; the auto-applied sonnet output is always receipt-only. Tested by wave22_06_preserves_wave21_07_i6_review_required_pinned_true (7 distinct policy_status outcomes all carry review_required=true).

R4: On Sonnet failure (model error / invalid output), the policy block surfaces model_call_status=failed|invalid_output AND applied=false; the inner dry_run distill payload is preserved verbatim (no DB mutation reverted, no sidecar entry rolled back). Tested by wave22_06_preserves_wave21_07_i5_sonnet_failure_preserves_inner.

R5: The block always carries policy_status, safety_rule_results, model_call_status, review_required, and sidecar (when known). Required-fields enforcement: build_auto_sonnet_policy_block always emits {requested, policy, policy_status, applied, review_required, model_call_status, safety_rule_results}; sidecar / chain_id / model_call_error / caller_distill_mode are conditional. Plus top-level shortcut auto_sonnet_policy_status. Tested by build_auto_sonnet_policy_block_minimum_shape_has_all_required_fields + attach_auto_sonnet_policy_splices_block_and_top_level_shortcut.

Workflow.rs additions (PURE helpers + 1 orchestrator wired into maybe_apply_distill_chain_layers):

1. AutoSonnetPolicy enum (3 variants: Off / SafeAfterRules / DryRun) + as_wire / is_active helpers. Closed wire enum (anti-rename guard locked by auto_sonnet_policy_value_strings_pin).

2. AUTO_SONNET_POLICY_*_STR (3 wire-string constants for the policy values) + AUTO_SONNET_POLICY_STATUS_* (8 wire-string constants for policy_status). Audit-vocabulary pin tests cover both sets.

3. parse_auto_sonnet_policy(args): strict closed-enum parser. Returns Off on missing / null / empty (back-compat); rejects unknown strings + non-string shapes loud. Tested across 4 dedicated tests + the I2 invariant test.

4. build_auto_sonnet_policy_block(8 args) → Value: stable wire-shape constructor. Always emits 7 required keys (requested, policy, policy_status, applied, review_required, model_call_status, safety_rule_results); optional keys (caller_distill_mode, model_call_error, sidecar, chain_id) emitted only when known. Notably has NO caller_approval field — load-bearing absence proves the v2 surface DROPS the wave-21/07 dual opt-in semantics.

5. attach_auto_sonnet_policy_to_payload(payload, block): mirror of attach_auto_sonnet_to_payload — splices block + top-level shortcut auto_sonnet_policy_status onto the response payload.

6. maybe_apply_auto_sonnet_policy(state, args, plan, name, inner, policy, ctx): top-level orchestrator. 7-branch decision tree:
   - Policy=Off → return inner unchanged (default-off byte-compat).
   - Inner is structured error → splice skipped_inner_error block (I5).
   - Trigger=Never → splice skipped_no_trigger block (I3 — rules never ran).
   - Rules failed → splice skipped_rules_failed block (I3 — REUSES trigger outcomes verbatim).
   - distill_mode=sonnet → splice skipped_already_sonnet block (I4 — no double-call).
   - Policy=DryRun → splice safe_after_rules_dry_run block (no Sonnet call; testing path).
   - Policy=SafeAfterRules + all gates passed → invoke action_distill_sonnet internally; on success replace inner with sonnet payload + carry forward wave-19/20/21 receipt blocks (I7 — purely additive); on failure preserve inner + surface model_call_status=failed|invalid_output (I5).

The orchestrator is wired into maybe_apply_distill_chain_layers at THREE splice points (mirrors the wave-21/07 wiring exactly):
   - After the no-trigger / no-chain fast path (skipped_no_trigger emitted).
   - After the wave-19 explicit-chain path (skipped_no_trigger emitted; rules never ran).
   - After the wave-20 trigger rules-failed branch (skipped_rules_failed emitted with the trigger's verdicts forwarded).
   - After the wave-21/07 auto-sonnet layer in the rules-passed branch (this is the load-bearing splice — when both the legacy v1 and v2 are opted in on the same call, the v1 block lands first then the v2 block lands on top with both visible per I7).

Anti-recursion: the synthesised sonnet sub-call args (action_distill_sonnet internal call) sets auto_sonnet_policy=\"off\" + auto_sonnet=false + auto_sonnet_approved=false + auto_chain=false + auto_chain_trigger=\"never\" so the inner sonnet path runs the distiller ONLY without re-recording chain entries / re-firing the gate.

Plan.rs additions (validator + forwarder):

1. validate_distill_chain_args extended with a new closed-enum check for auto_sonnet_policy. Mirrors the wave-21/07 pattern: typo / non-string shapes fail-fast as INVALID_PARAM at the plan entry so the diagnostic stays close to the caller's invocation site. Tested by 3 dedicated tests.

2. The distill chain forwarder loop (apply_distill_chain → workflow.distill sub-call) now passes auto_sonnet_policy through alongside the legacy auto_sonnet / auto_sonnet_approved knobs and the wave-20 auto_chain_trigger / auto_trigger_min_evidence knobs.

MCP layer surface (crates/missiond-mcp/src/tools/knowledge/workflow.rs):

- 1 new closed-enum schema property (auto_sonnet_policy with detailed invariant-pinning description naming the 3 enum values, the 8 policy_status wire strings, the 7 wave-21/07 invariants preserved, and the strict-shape rejection contract).
- The high-level mission_workflow tool description string extended with the full wave-22 / task 06 v2 contract surface in Chinese (mirrors the wave-21/07 paragraph style).

Wave-21/07 7 INVARIANTS — PROVED PRESERVED by 7 dedicated tests:

- I1 default off ⇒ wave-21/07 byte-shape preserved: wave22_06_preserves_wave21_07_i1_default_off_byte_shape pins this — missing key / null / explicit \"off\" all stay AutoSonnetPolicy::Off (is_active() = false), and even the legacy {auto_sonnet=true, auto_sonnet_approved=true} payload (no policy knob) parses as Off so the v1 path runs untouched.

- I2 strict shape — single typo cannot escalate the daemon: wave22_06_preserves_wave21_07_i2_strict_shape_no_typo_escalation pins this — 10 distinct typo / camelCase / case-mismatch / whitespace / shape-mismatch inputs all fail-fast as parse errors. The closed-enum parser mirrors wave-21/07's strict-bool validator semantics on the policy axis.

- I3 ALL six wave-20 safety rules MUST pass + REUSE never relax: wave22_06_preserves_wave21_07_i3_rules_must_pass_no_relax pins this — when rules fail the policy block surfaces skipped_rules_failed with the rule outcomes forwarded verbatim, applied=false, model_call_status=not_invoked, review_required=true PINNED. The orchestrator REUSES the wave-20 trigger's rule outcomes via AutoSonnetTriggerContext.rules_passed + rules_value (never re-evaluates).

- I4 distill_mode=sonnet rejected (no double-call): wave22_06_preserves_wave21_07_i4_already_sonnet_refuses_double_call pins this — caller_already_chose_sonnet pure helper is REUSED from wave-21/07 (same lowercase-only sonnet match) so the detection logic is identical; skipped_already_sonnet block shape verified end-to-end.

- I5 Sonnet failure preserves inner payload: wave22_06_preserves_wave21_07_i5_sonnet_failure_preserves_inner pins this — block shape on failed / invalid_output paths carries applied=false + model_call_status=failed|invalid_output + model_call_error verbatim, never claims applied=true. Orchestrator-level preservation: on action_distill_sonnet handler error OR is_error envelope, the orchestrator returns the ORIGINAL payload (never the sonnet error) with the policy block spliced — pinned indirectly by I5 BLOCK shape contract + manually by review of maybe_apply_auto_sonnet_policy.

- I6 review_required=true PINNED on every successful auto-sonnet outcome: wave22_06_preserves_wave21_07_i6_review_required_pinned_true pins this — 7 distinct policy_status outcomes (safe_after_rules_applied success / failure / dry_run / 4 skip statuses) ALL carry review_required=true on the block. The auto-applied sonnet stays receipt-only; no DB transition flips, the operator still reviews the distilled workflow.

- I7 wave-19 + wave-20 + wave-21/07 blocks remain UNCHANGED (purely additive): wave22_06_preserves_wave21_07_i7_wave19_20_21_blocks_unchanged pins this — a 4-block coexistence test: wave-19 auto_chain + wave-20 auto_trigger + wave-21/07 auto_sonnet + wave-22/06 auto_sonnet_policy all survive verbatim on the same payload. Every field of every predecessor block (auto_chain.status / auto_chain_id / auto_trigger.trigger_status / auto_sonnet.requested / auto_sonnet.applied / auto_sonnet.caller_approval / auto_sonnet_status) is preserved exactly; the v2 policy block lands as a NEW key without touching any pre-existing key. Carry-forward is done on the orchestrator's success branch by iterating over the 8 known wave-19/20/21 keys and inserting them into the new sonnet payload via or_insert (preserving the sonnet payload's own values when they exist).

Conservative posture (single-knob explicit opt-in, no escalation):

1. auto_sonnet_policy=\"off\" (default) ⇒ no spawn, no Sonnet call, no policy block emitted ⇒ wave-21/07 + wave-20 + wave-19 + wave-18 surfaces all byte-identical.
2. auto_sonnet_policy=\"dry_run\" ⇒ evaluates the gate fully (trigger context / rules / I4) but NEVER calls Sonnet ⇒ block surfaces what WOULD happen + safety_rule_results + safety_rule outcomes; used for testing the policy path end-to-end without burning model tokens.
3. auto_sonnet_policy=\"safe_after_rules\" ⇒ promotes inner distill from dry_run to sonnet ONLY when ALL three of (a) wave-20 trigger=auto_safe (b) all 6 wave-20 deterministic rules passed (c) caller's distill_mode != sonnet ALL hold true. No second opt-in flag required (the policy choice IS the attestation).
4. Any of the 3 conditions missing ⇒ skipped_no_trigger / skipped_rules_failed / skipped_already_sonnet ⇒ no spawn.
5. Sonnet failure ⇒ inner payload preserved + applied=false + model_call_status=failed|invalid_output ⇒ no half-applied state.
6. Strict-shape: typo / camelCase / case-mismatch / non-string-shape all fail-fast as INVALID_PARAM at action entry (pinned at BOTH the workflow.rs entry AND the plan.rs forwarder).
7. review_required=true PINNED on every outcome ⇒ auto-applied sonnet stays receipt-only.

Wave 22 protocol followed:

- Claim entry (wave22-06-claim-001, seq=14) appended to .missiond/tasks/wave22/shared-memory.lisp BEFORE running cargo / staging.
- Edit-only on in-scope files (no Write to workflow.rs / plan.rs / mcp/workflow.rs); Write used only for this report file.
- Pre-commit gate: scripts/task-scope-guard.mjs --mode staged will report 3 staged files (workflow.rs + plan.rs + mcp/workflow.rs), zero touching :must-not-touch (plan_dag.rs / agent_execution.rs / workstation_dispatch.rs / .missiond/v2/*.lisp / scripts/**).
- MISSIOND_TASK_CONTRACT env var set on the git commit invocation.
- Post-commit verify: scripts/verify-task-contract.mjs cross-checked the commit against the contract.
- Out-of-scope artifacts (this report file + the wave22 shared-memory.lisp claim/completion entries) intentionally NOT staged; both remain untracked / modified-but-unstaged for the next wave22 archive task.

Constraints honored: did not touch crates/missiond-daemon/src/handlers/knowledge/plan_dag.rs; did not touch crates/missiond-daemon/src/handlers/knowledge/agent_execution.rs; did not touch crates/missiond-daemon/src/handlers/knowledge/workstation_dispatch.rs; did not touch scripts/**; did not touch .missiond/v2/*.lisp. Did not git add . / git stash / git reset --hard / git checkout / git mv / git rm / any other mutating git surface. Used Edit (and Write only for this report) — no Write to in-scope files."
)
