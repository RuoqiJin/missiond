;; Wave 21 / Task 08 — Machine contract autonomous loop smoke v3.
;; Schema: missiond.report-contract.v1
;; Source: .missiond/tasks/wave21/wave21-08-machine-contract-autonomous-loop-smoke-v3.lisp

(report wave21-08-machine-contract-autonomous-loop-smoke-v3
  :schema "missiond.report-contract.v1"
  :task_id "wave21-08-machine-contract-autonomous-loop-smoke-v3"
  :status done
  :commit_hash "8ba872365a4b"
  :files_changed
    ["crates/missiond-daemon/src/handlers/knowledge/unified_entry.rs"
     "crates/missiond-daemon/src/handlers/knowledge/plan.rs"
     "crates/missiond-daemon/src/handlers/knowledge/workstation_dispatch.rs"
     "crates/missiond-daemon/src/handlers/knowledge/agent_execution.rs"]

  :acceptance_results
    [(:command "cargo test -p missiond-daemon handlers::knowledge::unified_entry::tests"
      :exit_code 0
      :ok true
      :notes "67 passed; 0 failed (64 baseline + 3 new wave21-08 tests). New tests: smoke_wave21_machine_contract_autonomous_loop_envelope_pins_every_invariant (single envelope round-trip pinning row-id pointers + machine-contract splice (task_contract_path == task_contract_source_path SSOT) + Wave 21/04 workstation_proposals applied=false / auto_spawn=false (I3-04) + Wave 21/06 LLM auto-approve decision != rejected / requires_human=true / applied=false (I1-06 + I3-06) + Wave 21/07 default-off byte-shape (no auto_sonnet block when not requested) (I1-07) + Wave 19/20 distill_chain triggered=true status=recorded chain_mode=record_only (I3-07) + v0 non-goals loud); smoke_wave21_llm_auto_approve_destructive_action_pins_requires_human (destructive action `archive` pins requires_human=true regardless of model output (I2-06) + destructive_check sourced from is_destructive_review_action not model (I5-06)); smoke_wave21_machine_contract_loop_proof_markdown_is_non_load_bearing (task_brief_preview MUST stay on inner payload, NOT in artifact_refs; brief preview still surfaces ## Source contract preamble on inner payload).")
     (:command "cargo test -p missiond-daemon handlers::knowledge::agent_execution::tests"
      :exit_code 0
      :ok true
      :notes "108 passed; 0 failed (102 baseline + 6 new wave21-08 tests). New tests: smoke_wave21_machine_contract_autonomous_loop_verifier_accepts_aligned_triple (wave21-03 happy path: aligned (contract, report, hash) triple passes verifier + summary echoes resolved paths + :checked surfaces all 5 wave21-03 rules); smoke_wave21_malformed_report_task_id_yields_structured_failure (mismatched :task_id MUST surface TASK_REPORT_TASK_ID_MISMATCH); smoke_wave21_malformed_task_contract_yields_structured_failure (schema mismatch MUST surface TASK_CONTRACT_MALFORMED via the wave19-08 contract gate); smoke_wave21_missing_report_yields_structured_failure (missing report file MUST surface TASK_REPORT_REQUIRED); smoke_wave21_mismatched_commit_hash_yields_structured_failure (mismatched commit_hash MUST surface TASK_REPORT_COMMIT_HASH_MISMATCH); smoke_wave21_report_and_contract_projectors_extract_required_fields (read_report_summary extracts schema/task_id/commit_hash + read_task_contract_id extracts head id + cross-check anchor: contract head id == report :task_id).")
     (:command "cargo test -p missiond-daemon"
      :exit_code 0
      :ok true
      :notes "1454 passed; 0 failed (1439 baseline + 15 net wave21-08 tests across the 4 :write-scope files: unified_entry +3 / agent_execution +6 / workstation_dispatch +3 / plan +3).")
     (:command "cargo build --workspace"
      :exit_code 0
      :ok true
      :notes "workspace clean build (only pre-existing warnings; no new warnings).")
     (:command "node scripts/check-architecture-lisp.mjs --all-v2"
      :exit_code 0
      :ok true
      :notes "20 v2 lisp files OK (no .missiond/v2/*.lisp touched per :must-not-touch).")
     (:command "git diff --check -- crates/missiond-daemon/src/handlers/knowledge/unified_entry.rs crates/missiond-daemon/src/handlers/knowledge/plan.rs crates/missiond-daemon/src/handlers/knowledge/workstation_dispatch.rs crates/missiond-daemon/src/handlers/knowledge/agent_execution.rs"
      :exit_code 0
      :ok true
      :notes "no whitespace errors in any of the four write-scope files.")]

  :scope_deviations []

  :notes
    "Wave21-08 lands the deterministic end-to-end smoke that closes the wave21 machine-contract autonomous loop. The smoke proves the four wave21-04..07 features (workstation LLM proposals propose-only / I3-04; plan inference apply-gate / I1-05..I6-05; LLM auto-approve propose-only / I1-06..I5-06; sonnet distill chain auto-apply / I1-07..I7-07) survive a single envelope round trip through the unified-entry decorator AND the wave21-03 verifier ratifies an aligned (contract, report, hash) triple end-to-end without parsing the inner JSON. No LLM, no spawn, no shell, no markdown read — every receipt is synthesised in JSON form or projected through the existing pure parsers (parse_task_contract / read_report_summary / read_task_contract_id / parse_workstation_proposals / classify_proposal_safety / build_distill_chain_block / build_workstation_dispatch_response). The 15 new tests group by handler:

unified_entry.rs (+3): the canonical machine-contract envelope round-trip + the destructive-action proposal short-circuit + the markdown-non-load-bearing pin. Together they prove the unified envelope surfaces task_contract_source_path == task_contract_path (SSOT invariant — observers can drive the loop entirely from artifact_refs), the wave21-04 workstation_proposals bundle auto_spawn=false invariant, the wave21-06 LLM auto-approve I1 (decision != rejected) + I2 (destructive_blocked) + I3 (applied=false + requires_human=true) + I5 (destructive_check sourced from is_destructive_review_action) invariants, the wave21-07 I1 default-off byte-shape (no auto_sonnet block when not requested), and the wave-19/20 distill_chain receipt byte-shape stays unchanged. The destructive-action smoke specifically pins that even when Sonnet suggests `approved` for an `archive` action, the bundle MUST surface requires_human=true and destructive_check MUST come from the deterministic check (not the model).

plan.rs (+3): exercise build_distill_chain_block in two skip-vs-warning shapes + build_workstation_dispatch_response in machine mode. The skip path proves the wave21-07 I3 invariant (the gate REUSES wave-20 trigger outcomes and MUST NOT fabricate evidence_path / chain_index_in_plan / distill_result on the skip branch). The warning path proves the wave-19/20/I7-07 additive contract — every optional surfaces verbatim when supplied. The machine-dispatch path proves task_contract_source_path surfaces the resolved on-disk path AND the brief preview carries the wave-19/07 ## Source contract preamble naming the same path.

workstation_dispatch.rs (+3): exercise the wave21-04 propose-only pipeline (parse_workstation_proposals + classify_proposal_safety + WorkstationProposalBundle::to_response_json) + the unavailable-no-fallback contract + the contract overlay → brief renderer chain. The propose-only smoke pins applied=false on every proposal across the safety spectrum (Safe target / InvalidStrategy / Safe objective) AND the bundle-level auto_spawn=false (I3-04 + I4-04 + I5-04). The unavailable smoke pins that even on Sonnet outage the bundle surfaces status=llm_unavailable + reason naming `no fallback` / `claude -p` / `prompt mode` (no silent degradation to deterministic). The overlay smoke pins parse_task_contract → overlay_contract → build_task_brief_with_source: contract :goal beats stale caller objective + :scope wins + :write-scope replaces stale owned_files + :must-not-touch surfaces verbatim + :commit :policy reaches hints + :dispatch-strategy beats caller fresh-code-alignment + brief carries ## Source contract preamble + agent-team hint exactly once.

agent_execution.rs (+6): exercise the wave21-03 verified-completion gate end-to-end against fixture task-contract / report-contract Lisp text on disk (tempdir-anchored, no real git, no real LLM). The happy-path smoke proves the verifier accepts an aligned (contract, report, hash) triple and surfaces every wave21-03 rule on the validation summary (preconditions_present + task_report_loadable + task_report_schema + task_id_matches_contract + commit_hash_matches_report). The four fail-fast smokes prove every malformed input maps to a deterministic structured-error code: missing report file → TASK_REPORT_REQUIRED; report :task_id mismatch → TASK_REPORT_TASK_ID_MISMATCH; commit_hash mismatch → TASK_REPORT_COMMIT_HASH_MISMATCH; malformed contract (schema mismatch) → TASK_CONTRACT_MALFORMED via the wave19-08 contract gate. The projector smoke pins that read_report_summary + read_task_contract_id extract the cross-check fields verbatim AND the contract head id equals the report :task_id (the cross-check anchor that wave21-03 compares).

Cross-wave invariants pinned in this single smoke battery:
  * I1-06 (LLM auto-approve proposal NEVER carries decision=rejected — validator demotes to needs_changes)
  * I2-06 (destructive actions short-circuit to destructive_blocked + pin requires_human=true)
  * I3-06 (every LLM auto-approve proposal carries applied=false + requires_human=true in v0)
  * I5-06 (destructive_check ALWAYS sourced from is_destructive_review_action, NOT model)
  * I3-04 (every workstation proposal carries applied=false + bundle pins auto_spawn=false)
  * I4-04 (Sonnet unavailable surfaces status=llm_unavailable + reason naming no-fallback contract)
  * I5-04 (classify_proposal_safety tags UnsupportedTarget / InvalidStrategy verbatim — never silently demotes)
  * I1-07 (auto_sonnet default-off byte-shape — no auto_sonnet block / shortcut when not requested)
  * I3-07 (distill chain gate REUSES wave-20 trigger outcomes — skip path MUST NOT fabricate applied-side fields)
  * I7-07 (distill chain block is purely additive — every optional surfaces verbatim when supplied)
  * Wave-19 task-contract emit (task_contract_path / task_contract_eligible / task_contract_mode / render_command surface verbatim)
  * Wave-20/04 dispatch-contract mode (dispatch_contract_mode=machine + task_contract_source_path SSOT)
  * Wave-21/03 verifier 5-rule cross-check (preconditions_present + task_report_loadable + task_report_schema + task_id_matches_contract + commit_hash_matches_report)

Markdown is non-load-bearing throughout: the task_brief_preview MUST NOT be projected into artifact_refs by the unified-entry decorator (machine-mode brief is rendered for human consumption only — observers consume task_contract_source_path as the SSOT pointer). The brief preview still lives on the inner payload so a human reading the response can see the rendered form, but the loop's invariants are entirely driven by the structured task-contract / report-contract files. This proves the wave21 autonomous loop can run end-to-end without ANY markdown read by any pipeline component — the contract files are the SSOT, the daemon-side pure helpers do all the cross-checks, and the unified envelope projection lifts only the load-bearing keys. Future waves layering additional autonomous-loop features can reuse this smoke battery as the regression baseline: any new feature that breaks an existing invariant or fabricates a non-load-bearing key in artifact_refs lands an explicit failure here. Wave21-09 hand-off: the lisp backfill task picks up the wave21 status on the V2 lisp authority files; this smoke is the SSOT proof that the wave21 features hold together at the source level."
)
