;; Wave 20 / Task 08 — Review auto-answer policy v0.
;; Schema: missiond.report-contract.v1
;; Source: .missiond/tasks/wave20/wave20-08-review-auto-answer-policy-v0.lisp

(report wave20-08-review-auto-answer-policy-v0
  :schema "missiond.report-contract.v1"
  :task_id "wave20-08-review-auto-answer-policy-v0"
  :status done
  :commit_hash "8adb0a85cda23b1ec1b43f324d31e42db4e2a8bd"
  :files_changed
    ["crates/missiond-daemon/src/handlers/knowledge/review_gate.rs"
     "crates/missiond-daemon/src/handlers/knowledge/unified_entry.rs"]

  :acceptance_results
    [(:command "cargo test -p missiond-daemon handlers::knowledge::review_gate::tests"
      :exit_code 0
      :ok true
      :notes "152 passed (122 baseline + 30 new wave-20/08 tests covering parse_auto_answer_policy default/recognised/case/unknown, auto_answer_policy_was_explicit, label round-trip for AutoAnswerPolicy and AutoAnswerStatus, destructive-action recogniser, evaluate_auto_answer_policy off/deterministic_safe/dry_run paths including blocked_when_protected/non_deterministic/additional_blocker/caller_decision, dry_run_always_defers, dry_run_destructive_still_surfaces_rule, never_returns_rejected (invariant I1), never_promotes_destructive (invariant I2), never_calls_llm (invariant I3 via sync signature + determinism check), skipped_block_carries_full_audit (invariant I4), and stamp_auto_answer_payload under off/auto_answered/skipped_destructive/dry_run_preview + invariant-I1 round-trip)")
     (:command "cargo test -p missiond-daemon handlers::knowledge::unified_entry::tests"
      :exit_code 0
      :ok true
      :notes "64 passed (58 baseline + 6 new wave-20/08 tests: build_plan_execute_args_forwards_wave20_auto_answer_policy across all three modes, build_plan_execute_args_wave20_auto_answer_policy_absent_when_omitted, build_artifact_refs_lifts_wave20_auto_answer_policy_fields, build_artifact_refs_omits_wave20_auto_answer_policy_when_inner_silent, smoke_wave20_auto_answer_policy_dry_run_envelope, smoke_wave20_auto_answer_policy_destructive_action_never_promotes)")
     (:command "cargo test -p missiond-daemon"
      :exit_code 0
      :ok true
      :notes "1288 passed (baseline 1252 + 36 new = 30 review_gate + 6 unified_entry)")
     (:command "cargo test -p missiond-mcp --lib"
      :exit_code 0
      :ok true
      :notes "17 passed")
     (:command "cargo build --workspace"
      :exit_code 0
      :ok true
      :notes "workspace clean build (78 baseline warnings unchanged)")
     (:command "node scripts/check-architecture-lisp.mjs --all-v2"
      :exit_code 0
      :ok true
      :notes "20 v2 lisp files OK")
     (:command "git diff --check -- crates/missiond-daemon/src/handlers/knowledge/review_gate.rs crates/missiond-daemon/src/handlers/knowledge/unified_entry.rs"
      :exit_code 0
      :ok true
      :notes "no whitespace errors in scoped files")
     (:command "node scripts/check-task-memory.mjs .missiond/tasks/wave20/shared-memory.lisp"
      :exit_code 0
      :ok true
      :notes "shared-memory check OK (1 ledger, 12 entries — wave20-08 claim appended pre-commit; completion appended post-verify)")
     (:command "node scripts/verify-task-contract.mjs .missiond/tasks/wave20/wave20-08-review-auto-answer-policy-v0.lisp"
      :exit_code 0
      :ok true
      :notes "post-commit verifier OK against commit 8adb0a85cda2")]

  :scope_deviations []

  :notes
    "wave20-08 lands an explicit deterministic auto-answer policy for the wave-16/02 review-question listener path. Default off preserves the wave-15..19 byte-shape exactly; the new knob lives entirely on the LISTENER side (controls when the listener may auto-answer Approved without a human reviewer) and is orthogonal to the wave-18/07 review_automation_policy (which controls the manager-action resolution surfaces). Three modes (off | deterministic_safe | dry_run) parsed through `parse_auto_answer_policy` (case-insensitive + trimmed; hyphenated `deterministic-safe` / `dry-run` aliases accepted; unknown values silently collapse to off so the response always echoes the resolved policy on `auto_answer_policy`). Hard invariants pinned by tests: I1 NEVER auto-reject (selected_decision can never be `rejected` under any mode; even when the wave-18/07 inspector degrades the suggestion under blocking rules we surface `needs_changes`); I2 destructive actions (archive | supersede | remove, case-insensitive, centralised in `DESTRUCTIVE_REVIEW_ACTIONS`) NEVER auto-promote even when every safety rule passes — a dedicated `AutoAnswerStatus::SkippedDestructiveAction` lets observers grep the rule trail; I3 pure / sync / never calls an LLM (signature enforcement + determinism check); I4 when skipped (any non-Off mode that did not reach AutoAnswered) the response carries `policy_result`, `selected_decision`, `safety_rule_results[]`, and `requires_human=true`. Safety rules inherited from wave-18/07: deterministic_mode (LLM/sonnet artefact blocks), no_file_write OR matching expected_file_sha256, no protected_source_or_target, no additional_blockers, caller_opt_in. New wave-20/08 rules: destructive_action / non_destructive_action surface on every non-Off evaluation; caller_decision_present blocker fires whenever an explicit review_decision was supplied (human authority always wins). dry_run mode evaluates the same inspector + guards but ALWAYS sets `requires_human=true` so operators can validate the policy outcome shape on a real review question without the policy mutating any DB row. unified_entry wires the new arg through the plan-execute branch verbatim (the pipeline never re-validates — the inner handler owns the strict allowlist) and `build_artifact_refs` lifts the five outcome keys (`auto_answer_policy / policy_result / selected_decision / safety_rule_results / requires_human`) into the unified envelope so a downstream consumer can read the listener-path policy outcome without re-parsing the inner JSON. Default off keeps the byte-identical wave-15..19 envelope (the lift only fires when the inner handler stamped a key). NO new MCP tool, NO live LLM auto-approval (invariant I3), NO destructive auto-promotion (invariant I2), NO autonomous workstation dispatch (the v0/v1 non-goal stays loud on every envelope). Doc-comment update on unified_entry pivots the legacy 'auto-answer a review question' non-goal to 'auto-answer a review question via LLM' so the new opt-in deterministic mode is explicitly documented while preserving the LLM ban.")
