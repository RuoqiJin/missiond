;; Wave 20 / Task 06 — cross-plan distill auto-trigger v1.
;; Schema: missiond.report-contract.v1
;; Source: .missiond/tasks/wave20/wave20-06-cross-plan-distill-auto-trigger-v1.lisp

(report wave20-06-cross-plan-distill-auto-trigger-v1
  :schema "missiond.report-contract.v1"
  :task_id "wave20-06-cross-plan-distill-auto-trigger-v1"
  :status done
  :commit_hash "3669ebc0bc12f4362f7d86657e90654cae38f06f"
  :files_changed
    ["crates/missiond-daemon/src/handlers/knowledge/workflow.rs"
     "crates/missiond-mcp/src/tools/knowledge/workflow.rs"]

  :acceptance_results
    [(:command "cargo test -p missiond-daemon handlers::knowledge::workflow::tests"
      :exit_code 0
      :ok true
      :notes "127 passed (20 wave-20 auto-trigger tests added on top of wave-19 baseline)")
     (:command "cargo test -p missiond-daemon"
      :exit_code 0
      :ok true
      :notes "1180 passed (baseline 1160 + 20 wave-20 tests)")
     (:command "cargo test -p missiond-mcp --lib"
      :exit_code 0
      :ok true
      :notes "17 passed (workflow schema additions exercised via test_workflow_actions_match_lisp)")
     (:command "cargo build --workspace"
      :exit_code 0
      :ok true
      :notes "workspace clean build")
     (:command "node scripts/check-architecture-lisp.mjs --all-v2"
      :exit_code 0
      :ok true
      :notes "20 v2 lisp files OK")
     (:command "git diff --check -- crates/missiond-daemon/src/handlers/knowledge/workflow.rs crates/missiond-mcp/src/tools/knowledge/workflow.rs"
      :exit_code 0
      :ok true
      :notes "no whitespace errors")]

  :scope_deviations []

  :notes
    "Auto-trigger v1 layered ON TOP of wave-19 auto_chain. New arg auto_chain_trigger ∈ {never (default), auto_safe}. Six deterministic safety rules evaluated in fixed order: inner_distill_succeeded, distill_mode_recorded, project_root_resolved, evidence_sidecar_present, evidence_min_entries, chain_id_not_already_recorded. ALL pass → falls through to wave-19 maybe_apply_auto_chain (synthesises auto_chain=true args view); response gains auto_trigger block + top-level auto_trigger_status / auto_trigger_chain_id shortcuts alongside the wave-19 auto_chain block. ANY fail → trigger_status=skipped_rules_failed with full safety_rule_results, no chain row appended (atomic refusal). Sonnet remains explicit-only — the trigger only enables the record-only auto-chain hook; distill_mode=sonnet is the sole path to invoke the distiller actor. Backward compatibility: when auto_chain_trigger is missing/null/blank/'never' AND auto_chain is missing/false, response is byte-identical with the wave-19 distill payload (verified by parse_auto_chain_trigger_default_never_and_explicit_modes + auto_chain_requested_default_false_and_explicit_true). Inner distill error envelopes short-circuit with trigger_status=skipped_inner_error so audit can tell rule failure apart from upstream distill failure (mirrors plan.rs apply_distill_chain policy).")
