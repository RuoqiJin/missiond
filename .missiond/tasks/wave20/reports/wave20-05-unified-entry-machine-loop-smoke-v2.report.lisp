;; Wave 20 / Task 05 — Unified entry machine loop smoke v2.
;; Schema: missiond.report-contract.v1
;; Source: .missiond/tasks/wave20/wave20-05-unified-entry-machine-loop-smoke-v2.lisp

(report wave20-05-unified-entry-machine-loop-smoke-v2
  :schema "missiond.report-contract.v1"
  :task_id "wave20-05-unified-entry-machine-loop-smoke-v2"
  :status done
  :commit_hash "d308fae8a9867c55a3d418793f158d1d674c5d43"
  :files_changed
    ["crates/missiond-daemon/src/handlers/knowledge/unified_entry.rs"]

  :acceptance_results
    [(:command "cargo test -p missiond-daemon handlers::knowledge::unified_entry::tests"
      :exit_code 0
      :ok true
      :notes "58 passed (52 baseline + 6 new wave-20/05 smoke tests: build_artifact_refs_lifts_wave19_20_machine_contract_fields, build_artifact_refs_omits_machine_contract_keys_when_inner_silent, smoke_wave20_machine_loop_fixture_contract_overlays_hints, smoke_wave20_machine_loop_canonical_contract_handoff, smoke_wave20_machine_mode_markdown_brief_is_non_load_bearing, smoke_wave20_machine_mode_malformed_contract_refuses_no_prompt_fallback)")
     (:command "cargo test -p missiond-daemon"
      :exit_code 0
      :ok true
      :notes "1252 passed (baseline 1246 + 6 new wave-20/05 tests in unified_entry::tests)")
     (:command "cargo build --workspace"
      :exit_code 0
      :ok true
      :notes "workspace clean build")
     (:command "node scripts/check-architecture-lisp.mjs --all-v2"
      :exit_code 0
      :ok true
      :notes "20 v2 lisp files OK")
     (:command "git diff --check -- crates/missiond-daemon/src/handlers/knowledge/unified_entry.rs crates/missiond-daemon/src/handlers/knowledge/plan.rs crates/missiond-daemon/src/handlers/knowledge/workstation_dispatch.rs"
      :exit_code 0
      :ok true
      :notes "no whitespace errors in scoped files (only unified_entry.rs modified; plan.rs / workstation_dispatch.rs untouched)")
     (:command "node scripts/check-task-contract.mjs --all"
      :exit_code 0
      :ok true
      :notes "26 task contracts OK")
     (:command "node scripts/check-task-memory.mjs .missiond/tasks/wave20/shared-memory.lisp"
      :exit_code 0
      :ok true
      :notes "shared-memory check OK (1 ledger, 11 entries — added wave20-05 claim + completion)")
     (:command "node scripts/task-scope-guard.mjs --task .missiond/tasks/wave20/wave20-05-unified-entry-machine-loop-smoke-v2.lisp --mode commit"
      :exit_code 0
      :ok true
      :notes "scope guard OK against commit d308fae8a986 (1 file in :write-scope, no :must-not-touch overlap)")
     (:command "node scripts/verify-task-contract.mjs .missiond/tasks/wave20/wave20-05-unified-entry-machine-loop-smoke-v2.lisp"
      :exit_code 0
      :ok true
      :notes "post-commit verifier OK against commit d308fae8a986")]

  :scope_deviations []

  :notes
    "wave20-05 lands a deterministic, no-LLM, no-spawn smoke proving the wave-20/04 machine-driven dispatch loop survives end-to-end through the unified entry. Two layers ship together: (1) build_artifact_refs is extended to lift the wave-19/06 + wave-20/04 machine-contract splice — task_contract_mode, task_contract_eligible, task_contract_path, task_contract_source_path, dispatch_contract_mode, render_command, task_contract_skip_reason, task_contract_error — so the unified-entry envelope preserves the machine handoff without callers diving back into the inner JSON; (2) six new tests pin the smoke contract. Smoke stages covered: (s4) plan-compile dry_run with the wave-20/05 fixture board task, (s6) plan-execute machine mode (`task_contract_mode=\"emit\"` + `dispatch_contract_mode=\"machine\"` + `render_markdown=false`) — fixture task.lisp text is parsed via parse_task_contract → WorkstationDispatchHints::overlay_contract → build_task_brief_with_source to prove the contract-driven brief renders without IO. Machine-contract field survival: every key is asserted on artifact_refs after `decorate`, with the SSOT invariant `task_contract_source_path == task_contract_path` pinned on the envelope shape. Markdown non-load-bearing proof: task_brief_preview lives ONLY on the inner payload (NOT projected into artifact_refs); the test pins this with an explicit `assert!(!refs.contains_key(\"task_brief_preview\"))` plus a positive assertion that render_command IS lifted (so callers know how to produce the brief out of process if they want it). Malformed-contract refusal: parse_task_contract is called against intentionally-broken fixture text → returns TaskContractParseError::MissingRequired(\"goal\"); the synthesised SafeDescriptor refusal payload (mirroring build_workstation_dispatch_response in machine mode for the SafeDescriptor branch) is fed through decorate and asserts (a) workstation_dispatch_status stays `skipped_malformed_task_contract`, (b) no inner_result leaked, (c) no `claude -p` shell-out artefact appears anywhere on the payload. Wave-15..19 byte-compat preserved: a separate test (build_artifact_refs_omits_machine_contract_keys_when_inner_silent) pins that the new splice does NOT fabricate any machine-contract key when the inner handler is silent. No production code path mutated; no fallback / heuristic introduced — the smoke is pure projection + synthesised-payload contract testing.")
