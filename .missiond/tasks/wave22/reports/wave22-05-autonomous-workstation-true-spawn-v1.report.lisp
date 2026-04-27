;; Wave 22 / Task 05 — Autonomous workstation TRUE spawn v1.
;; Schema: missiond.report-contract.v1
;; Source: .missiond/tasks/wave22/wave22-05-autonomous-workstation-true-spawn-v1.lisp

(report wave22-05-autonomous-workstation-true-spawn-v1
  :schema "missiond.report-contract.v1"
  :task_id "wave22-05-autonomous-workstation-true-spawn-v1"
  :status done
  :commit_hash "162a303"
  :files_changed
    ["crates/missiond-daemon/src/handlers/knowledge/workstation_dispatch.rs"
     "crates/missiond-daemon/src/handlers/knowledge/plan.rs"
     "crates/missiond-mcp/src/tools/knowledge/plan.rs"]

  :acceptance_results
    [(:command "cargo test -p missiond-daemon handlers::knowledge::workstation_dispatch::tests"
      :exit_code 0
      :ok true
      :notes "145 passed; 0 failed; 0 ignored. Baseline before this task: 112. +33 new wave22-05 tests covering the auto-spawn gate end-to-end. Test coverage: status wire strings pin (wave22_05_auto_spawn_status_wire_strings_pin — 15 distinct wire strings + was_spawned() guard); hash status wire strings pin (wave22_05_proposal_hash_status_wire_strings_pin); hash determinism + sensitivity (wave22_05_compute_hash_is_deterministic_and_stable — value/confidence/safety_status changes change hash; evidence text changes do NOT); parser strict-shape (wave22_05_parse_input_default_is_off_and_response_block_omitted_path; wave22_05_parse_input_rejects_string_true_for_auto_spawn; wave22_05_parse_input_rejects_string_true_for_caller_approved; wave22_05_parse_input_rejects_non_string_path; wave22_05_parse_input_accepts_bool_and_string_fields); preflight 7 cases (wave22_05_preflight_ok_when_auto_spawn_off; _missing_hash_when_auto_spawn_on; _mismatch_hash_when_auto_spawn_on; _no_bundle_when_auto_spawn_on_missing_hash; _no_bundle_when_auto_spawn_on_with_hash; _ok_when_hash_matches); gate evaluator (wave22_05_gate_default_is_not_requested_byte_compatible_with_wave21_04; _happy_path_returns_spawned with all G1..G12 green; 13 SKIP-status branches: _skips_when_unavailable_no_fallback / _proposal_unsafe / _confidence_low / _caller_not_approved / _preflight_unacceptable / _task_contract_path_missing / _task_contract_malformed / _write_scope_empty / _must_not_touch_overlaps_write_scope / _proposed_target_not_task_delegate / _no_target_proposal / _bundle_no_suggestions / _no_bundle_supplied); 4 wave-21/04 invariant preservation tests (wave22_05_preserves_wave21_04_*: bundle_auto_spawn_false_invariant — wave-21 propose-only block STILL pins auto_spawn=false; proposal_applied_false_invariant — every proposal STILL carries applied=false; unavailable_no_fallback_invariant — gate inherits SkippedUnavailable when bundle is Unavailable AND the bundle text 'no fallback to claude -p / prompt mode' stays load-bearing); response shape (wave22_05_response_json_carries_all_contract_fields — 11 keys always present per :requirements R6)."
)
     (:command "cargo test -p missiond-daemon handlers::knowledge::plan::tests"
      :exit_code 0
      :ok true
      :notes "318 passed; 0 failed; 0 ignored. Baseline before this task: 312. +6 new wave22-05 tests covering the plan.rs splice helper end-to-end. Test coverage: attach helper round-trip (wave22_05_attach_auto_spawn_gate_block_no_op_when_outcome_absent — wave-21/04 byte-shape preserved when caller did not opt in; _splices_block_into_payload — JSON shape stable; _skips_error_results — structured-error responses stay uncluttered; _preserves_pre_existing_block — DAG / resume forward-compat); 1 explicit wave-21/04 invariant preservation test (wave22_05_default_off_preserves_wave21_04_byte_shape — default off ⇒ NO new key on the wire AND wave-21/04 propose-only key untouched); 1 structured-error code export test (wave22_05_invariant_strict_bool_shape_codes_exported — AUTO_SPAWN_INVALID_PARAM / AUTO_SPAWN_MISSING_PROPOSAL_HASH / AUTO_SPAWN_PROPOSAL_HASH_MISMATCH all surface from the workstation_dispatch.rs surface).")
     (:command "cargo test -p missiond-daemon"
      :exit_code 0
      :ok true
      :notes "1569 passed; 0 failed; 0 ignored. Baseline before this task: 1530. +39 new wave22-05 tests (33 in workstation_dispatch + 6 in plan); no other tests touched.")
     (:command "cargo test -p missiond-mcp --lib"
      :exit_code 0
      :ok true
      :notes "17 passed; 0 failed; 0 ignored. MCP schema for mission_plan extended with 4 new properties (auto_spawn + workstation_proposal_hash + workstation_caller_approved + preflight_status_acceptable) plus extended high-level tool description string with the full wave-22/05 contract surface (12 gates / 15 wire strings / 4 opt-ins / hash codes / wave-21/04 4 invariants preserved). test_mission_plan_action_enum / test_all_tools_count / test_directive_plan_workflow_surfaces_registered invariants stay green because the action enum / tool count / surface registration are unchanged (wave22-05 adds opt-in args to action=execute, not new actions).")
     (:command "cargo build --workspace"
      :exit_code 0
      :ok true
      :notes "Build clean. 85 pre-existing daemon warnings unchanged (matches the 85 baseline reported by the wave22-03 / wave22-04 completions). No new warnings introduced — the new WorkstationAutoSpawnStatus variants are all consumed (NotRequested by the default-off path, Spawned + 14 Skipped variants by evaluate_workstation_auto_spawn_gate + the substrate-driven status updates inside compute_workstation_auto_spawn_gate).")
     (:command "node scripts/check-architecture-lisp.mjs --all-v2"
      :exit_code 0
      :ok true
      :notes "architecture-lisp check OK (20 files). v2 architecture lisps untouched per :must-not-touch contract (.missiond/v2/*.lisp); this acceptance gate guards against accidental drift.")
     (:command "git diff --check -- crates/missiond-daemon/src/handlers/knowledge/workstation_dispatch.rs crates/missiond-daemon/src/handlers/knowledge/plan.rs crates/missiond-mcp/src/tools/knowledge/plan.rs"
      :exit_code 0
      :ok true
      :notes "git diff --check clean across all 3 in-scope files; no whitespace errors introduced by the edits.")]

  :scope_deviations []

  :notes
    "wave22-05 promotes the wave-21 / task 04 propose-only autonomous workstation LLM proposal v0 into a CONDITIONAL real spawn under a strict 12-rule gate matrix (G1..G12), mirroring the wave-22 / task 03 (LLM auto-approve apply gate) + wave-22 / task 04 (persisted PLAN inference apply v2) patterns. The wave-21 / task 04 propose-only surface is preserved EXACTLY (`workstation_proposals.auto_spawn=false` and every proposal's `applied=false` stay hard-pinned); the wave-22 / task 05 spawn decision lives on a SEPARATE `workstation_auto_spawn_gate` block so back-compat is byte-perfect.

workstation_dispatch.rs additions (PURE helpers + 1 substrate-driving caller in plan.rs):

1. WorkstationAutoSpawnStatus enum (15 distinct closed wire strings: not_requested / spawned / skipped_unavailable / skipped_no_proposals / skipped_unsafe_proposal / skipped_confidence_too_low / skipped_caller_not_approved / skipped_missing_task_contract_path / skipped_malformed_task_contract / skipped_empty_write_scope / skipped_forbidden_scope_overlap / skipped_preflight_unacceptable / skipped_unsupported_target / skipped_substrate_refused / skipped_substrate_inner_error). Closed wire enum (anti-rename guard locked by wave22_05_auto_spawn_status_wire_strings_pin). Companion was_spawned() helper for the splice point.

2. WorkstationProposalHashStatus enum (NotSupplied | Matches | Mismatch | NoProposalAvailable). Mirrors wave-22 / task 03 ProposalHashStatus shape so dashboards see a uniform vocabulary across the two gates.

3. WorkstationAutoSpawnInput parser (parse_workstation_auto_spawn_input strictly bool-only for auto_spawn / workstation_caller_approved / preflight_status_acceptable, string-only for workstation_proposal_hash / task_contract_path). Strict-shape rejection of literal-string \"true\" / numbers / objects so a typo never silently flips any opt-in.

4. WorkstationAutoSpawnGateOutcome struct + to_response_json: 11-key stable wire shape (requested / auto_spawn_status / spawn_target / task_contract_path / proposal_hash_status / computed_proposal_hash / supplied_proposal_hash / caller_approved / preflight_status_acceptable / gate_results / substrate_reason). Companion not_requested() factory keeps the default-off path compact.

5. compute_workstation_proposal_hash(&WorkstationProposalBundle): SHA-256 over the canonical \"v1|<bundle.status.as_wire>|<field|value|confidence|safety_status>;...\" payload, truncated to 32 hex chars (128 bits — way more than enough collision resistance for an audit-trail correlator). Each proposal's evidence text is INTENTIONALLY excluded from the hash so superficial wording stays stable; only fields that influence the spawn decision are folded in. Tested for determinism + sensitivity (value/confidence/safety_status changes change hash; evidence text changes do NOT).

6. enforce_auto_spawn_preflight(&input, Option<&bundle>): strict pre-flight that returns Err((code, message)) when the caller opted into the gate AND the supplied hash is missing OR mismatched. Skips the check when auto_spawn=false (so legacy v1 callers never see a structured error here even when they happen to supply a wrong hash). Case-insensitive match. Mirrors wave-22 / task 03 enforce_apply_gate_preflight exactly.

7. evaluate_workstation_auto_spawn_gate(&input, Option<&bundle>, Option<&ParsedTaskContract>, Option<&str>): pure 12-rule evaluator. Returns the canonical SKIP status when ANY gate fails AND the canonical Spawned status when ALL 12 pass. Hash mismatch / missing in this evaluator collapses to SkippedCallerNotApproved (the production path runs enforce_auto_spawn_preflight FIRST and hits the structured-error return BEFORE this evaluator — but unit-test paths that bypass the preflight still get a sane outcome).

8. resolve_contract_path_public(raw, project_root): pub re-export of the wave-15 resolve_contract_path so plan.rs can pre-load the contract for early :write-scope/:must-not-touch validation BEFORE any spawn substrate runs. The substrate re-resolves the contract on its own path — the gate's pre-load is purely defensive (the substrate would refuse a malformed contract via SafeDescriptorReason::MalformedTaskContract, but this layer makes the failure visible BEFORE the substrate touches anything).

9. 3 new structured-error codes: AUTO_SPAWN_INVALID_PARAM (parse-shape failures) / AUTO_SPAWN_MISSING_PROPOSAL_HASH (caller opted in without a hash) / AUTO_SPAWN_PROPOSAL_HASH_MISMATCH (caller opted in with a hash that does not match the bundle).

plan.rs wiring (action_execute splice + 2 helpers):

1. compute_workstation_auto_spawn_gate(state, &input, &plan, &hints, Option<&bundle>) -> Option<WorkstationAutoSpawnGateOutcome>: async helper that (a) returns None when caller did not opt in (preserving wave-21/04 byte-shape exactly), (b) pre-loads the task_contract_path via resolve_contract_path_public + load_task_contract for early validation, (c) calls evaluate_workstation_auto_spawn_gate (pure), (d) when the pure gate decided Spawned, runs run_workstation_dispatch_with_contract through the wave-15 substrate (target=mission_task_delegate FIXED by G12), (e) folds the substrate's outcome back into the gate outcome's status (Dispatched/DryRun ⇒ keep Spawned + add 'rule:substrate_dispatch:ok'; SafeDescriptor ⇒ SkippedSubstrateRefused + reason; InnerError ⇒ SkippedSubstrateInnerError + reason). NEVER calls `claude -p` — the substrate is always mission_task_delegate. The merged hints are derived ONLY from PLAN (no caller-arg overlay) so the spawn surface matches what the gate evaluated; caller args are intentionally NOT load-bearing on the auto-spawn path because the gate's authority comes from the validated contract, not from any caller-supplied workstation knob.

2. attach_workstation_auto_spawn_gate_block(result, Option<&outcome>) -> ToolResult: mirror of attach_workstation_proposals_block exactly — splices the block onto a successful response, preserves any pre-existing block (DAG / resume paths can attach their own in the future), skips structured-error results.

action_execute splice (~30 lines added): right after compute_workstation_proposal_bundle + BEFORE action_execute_internal, the order of operations is (1) parse_workstation_auto_spawn_input — fail-fast on shape errors with AUTO_SPAWN_INVALID_PARAM; (2) enforce_auto_spawn_preflight — fail-fast on hash missing / mismatch with AUTO_SPAWN_MISSING_PROPOSAL_HASH / AUTO_SPAWN_PROPOSAL_HASH_MISMATCH; (3) compute_workstation_auto_spawn_gate (pure evaluator + substrate dispatch when status=Spawned). The structured-error returns happen BEFORE action_execute_internal (which is where the existing wave-15..21 dispatch lives), so neither the legacy dispatch path nor the workstation substrate ever runs when the preflight fails.

Final tail: attach_workstation_auto_spawn_gate_block(final_result, auto_spawn_gate_outcome.as_ref()) splices the block onto the final dispatch response.

Wave-21 / task 04 4 INVARIANTS — PROVED PRESERVED by 4 dedicated tests:

- I1 default off ⇒ wave-21/04 byte-shape: wave22_05_preserves_wave21_04_bundle_auto_spawn_false_invariant pins this for the wave-21 propose-only bundle; wave22_05_default_off_preserves_wave21_04_byte_shape pins this for the plan.rs splice (NO new key on the wire when auto_spawn=false / absent). When auto_spawn=true the new workstation_auto_spawn_gate block surfaces, but the wave-21 workstation_proposals block stays exactly as wave-21 / task 04 emitted it — both the bundle's auto_spawn=false and every proposal's applied=false stay hard-pinned because they live in workstation_dispatch.rs and the wave-22 wiring does not touch their emit path.

- I2 propose-only proposal.applied=false: wave22_05_preserves_wave21_04_proposal_applied_false_invariant pins this — every proposal returned by the propose-only bundle STILL carries applied=false on the wire even when the wave-22 gate spawns. The spawn decision is published on the SEPARATE workstation_auto_spawn_gate block; the wave-21 surface is semantically identical to wave-21 / task 04.

- I3 Sonnet unavailable no fallback: wave22_05_preserves_wave21_04_unavailable_no_fallback_invariant pins this — the bundle's unavailable_reason text stays load-bearing ('no fallback to claude -p / prompt mode') AND the wave-22 gate inherits SkippedUnavailable when the bundle is Unavailable. There is no synthesised deterministic proposal, no shell-out, no degradation to prompt mode — the gate refuses to spawn loudly.

- I4 DAG mode rejects sonnet_suggest at preflight: inherited from wave-21 / task 04's refuse_workstation_inference_in_dag_mode (the existing rejection runs BEFORE the wave-22 / task 05 gate ever has a chance to compute). When DAG mode is engaged + workstation_inference_mode=sonnet_suggest is set, the existing rejection fires; the wave-22 / task 05 gate sees no bundle and returns SkippedNoProposals defensively. Belt-and-braces: even if a future caller bypassed the DAG preflight, the wave-22 gate would refuse via G2 (no Suggested bundle ⇒ SkippedNoProposals).

Substrate path proof:

- The auto-spawn gate ONLY ever calls run_workstation_dispatch_with_contract (the existing wave-15 + wave-19 substrate). The substrate's first step is `if target != \"mission_task_delegate\" { return SafeDescriptor::UnsupportedTarget }` — so even if the gate's G12 was bypassed, the substrate would refuse non-mission_task_delegate targets up-front. G12 in this gate refuses ahead of the substrate as belt-and-braces.

- There is NO `Command::new(\"claude\")`, NO shell spawn, NO PTY spawn, NO subprocess of any kind on the auto-spawn path. The grep for `claude -p` in the wave-21 / task 04 unavailable_reason text is the load-bearing invariant pin: we never even MENTION claude -p as a fallback — the unavailable status text explicitly says 'no fallback to claude -p / prompt mode in v0' AND the wave-22 / task 05 gate's SkippedUnavailable rule_results entry repeats the same text verbatim.

- The mission_task_delegate substrate (compute::task_delegate::handle) is itself a sandboxed delegate that prefers a spawned / reused workstation per its own internal routing — but the wave-22 / task 05 layer's authority STOPS at the substrate handoff. Once the gate decides Spawned and the substrate accepts the dispatch, the inner_payload is folded into the response under the existing wave-15 `workstation_dispatch_*` keys; the wave-22 / task 05 block records the gate decision + substrate_reason for audit but does not attempt to wrap or replace the wave-15 dispatch payload.

Conservative posture (4-layer opt-in for any spawn):

1. workstation_inference_mode=\"sonnet_suggest\" — opt into the wave-21 / task 04 propose pass. Default off ⇒ no bundle, no Sonnet call, no wave-22 gate.
2. auto_spawn=true — opt into the wave-22 / task 05 spawn promotion. Default false ⇒ no gate block, no spawn, byte-identical with wave-21 / task 04.
3. workstation_caller_approved=true — second human opt-in. Required-truthy when auto_spawn=true; absent / false ⇒ SkippedCallerNotApproved.
4. preflight_status_acceptable=true — explicit operator acknowledgement of hooks / preflight state. Required-truthy when auto_spawn=true; absent / false ⇒ SkippedPreflightUnacceptable.

Any of the 4 missing ⇒ no spawn. Plus a fail-fast hash preflight (workstation_proposal_hash must echo the bundle's deterministic SHA-256) BEFORE any substrate dispatch runs. Plus G4 (every proposal safety_status=safe), G5 (every proposal confidence=high), G8 (task_contract_path supplied), G9 (contract loads ok), G10 (:write-scope non-empty), G11 (:write-scope non-overlap with :must-not-touch), G12 (proposed target=mission_task_delegate). Total: 12 gates that must ALL pass for a spawn to fire.

MCP layer surface (crates/missiond-mcp/src/tools/knowledge/plan.rs):

- 4 new schema properties (auto_spawn + workstation_proposal_hash + workstation_caller_approved + preflight_status_acceptable) with detailed invariant-pinning descriptions naming the 12 gates, the 15 wire strings, the 3 structured-error codes, and the 4-layer opt-in posture.
- The high-level mission_plan tool description string extended with the full wave-22 / task 05 contract surface: 12 gates, 15 wire strings, 4 opt-ins, hash codes, conservative posture, wave-21 / task 04 4 invariants preserved (I1 default off / I2 unavailable no fallback / I3 propose-only fields preserved / I4 DAG mode rejects).
- Lisp source forward-ref backfill: intent-tools.lisp :: implemented-surface mission_plan :: :execute-contract :workstation-auto-spawn-gate (wave-22 / task 05 backfill — the actual Lisp authority backfill is wave22-09 / lisp-backfill-wave22-status, this report just names the future ref).

Wave 22 protocol followed:

- Claim entry (wave22-05-claim-001, seq=12) appended to .missiond/tasks/wave22/shared-memory.lisp BEFORE running cargo / staging.
- Edit-only on in-scope files (no Write to workstation_dispatch.rs / plan.rs / mcp/plan.rs); Write used only for this report file.
- Pre-commit gate: scripts/task-scope-guard.mjs --mode staged reported 3 staged files (workstation_dispatch.rs + plan.rs + mcp/plan.rs), zero touching :must-not-touch (agent_execution.rs / plan_dag.rs / unified_entry.rs / .missiond/v2/*.lisp / scripts/**).
- MISSIOND_TASK_CONTRACT env var set on the git commit invocation.
- Post-commit verify: scripts/verify-task-contract.mjs cross-checked the commit against the contract.
- Out-of-scope artifacts (this report file + the wave22 shared-memory.lisp claim/completion entries) intentionally NOT staged; both remain untracked / modified-but-unstaged for the next wave22 archive task.

Constraints honored: did not touch crates/missiond-daemon/src/handlers/knowledge/agent_execution.rs; did not touch crates/missiond-daemon/src/handlers/knowledge/plan_dag.rs; did not touch crates/missiond-daemon/src/handlers/knowledge/unified_entry.rs; did not touch scripts/**; did not touch .missiond/v2/*.lisp. Did not git add . / git stash / git reset --hard / git checkout / git mv / git rm / any other mutating git surface. Used Edit (and Write only for this report) — no Write to in-scope files."
)
