;; Wave 20 shared-memory ledger.
;; Schema: .missiond/tasks/schema/shared-memory-v1.lisp
;; Checker: scripts/check-task-memory.mjs
;;
;; Append-only. Agents add entries while they hold a live :claim for their
;; task id. Editing or removing prior entries is forbidden; append a
;; (correction ...) entry instead.

(shared-memory wave20
  :schema "missiond.shared-memory.v1"
  :wave wave20
  :created-at "2026-04-27T00:00:00+08:00"
  :sequence 1

  (observation
    :id wave20-bootstrap-001
    :task wave20-00-archive-wave19-task-contracts
    :agent codex-orchestrator
    :seq 1
    :at "2026-04-27T00:00:00+08:00"
    :touched []
    :summary "Bootstrap entry: Wave 20 will use shared-memory for scope coordination and task-contract verification handoff.")

  (claim
    :id wave20-02-claim-001
    :task wave20-02-renderer-scoped-commit-guard-v2
    :agent claudecode
    :seq 2
    :at "2026-04-27T01:00:00+08:00"
    :touched ["scripts/render-claudecode-task.mjs"
              ".missiond/tasks/schema/task-contract-v1.lisp"
              ".missiond/tasks/wave20/wave20-00-archive-wave19-task-contracts.lisp"
              ".missiond/claudecode/wave20-00-archive-wave19-task-contracts.md"]
    :summary "Render scoped staging + pre-commit guard sequence in ClaudeCode briefs whenever :commit :required is true.")

  (claim
    :id wave20-03-claim-001
    :task wave20-03-execution-preflight-contract-scope-v1
    :agent claudecode
    :seq 3
    :at "2026-04-27T02:00:00+08:00"
    :touched ["crates/missiond-daemon/src/handlers/knowledge/agent_execution.rs"
              "crates/missiond-mcp/src/tools/knowledge/agent_execution.rs"]
    :summary "Teach mission_execution preflight_commit to load task_contract_path (read-only) and project staged/changed files against contract :write-scope + :must-not-touch with new structured fields.")

  (claim
    :id wave20-06-claim-001
    :task wave20-06-cross-plan-distill-auto-trigger-v1
    :agent claudecode
    :seq 4
    :at "2026-04-27T03:00:00+08:00"
    :touched ["crates/missiond-daemon/src/handlers/knowledge/workflow.rs"
              "crates/missiond-mcp/src/tools/knowledge/workflow.rs"]
    :summary "Add opt-in auto_chain_trigger=auto_safe layer over wave-19 auto-chain; deterministic 6-rule safety set gates record-only fall-through; sonnet stays explicit-only.")

  (completion
    :id wave20-06-completion-001
    :task wave20-06-cross-plan-distill-auto-trigger-v1
    :agent claudecode
    :seq 5
    :at "2026-04-27T03:30:00+08:00"
    :touched ["crates/missiond-daemon/src/handlers/knowledge/workflow.rs"
              "crates/missiond-mcp/src/tools/knowledge/workflow.rs"]
    :summary "Wave-20 auto-trigger v1 landed. mission_workflow distill accepts auto_chain_trigger ∈ {never, auto_safe} (default never preserves wave-19 byte-compat). auto_safe runs 6 deterministic safety rules (inner_distill_succeeded, distill_mode_recorded, project_root_resolved, evidence_sidecar_present, evidence_min_entries, chain_id_not_already_recorded); ALL pass → falls through to wave-19 recorder + splices auto_trigger block (trigger_status, chain_id, sidecar, safety_rule_results) + top-level shortcuts; ANY fail → trigger_status=skipped_rules_failed surfacing all rule outcomes, no chain row appended. Sonnet still gated solely by distill_mode=sonnet — trigger only enables the record-only auto-chain hook. daemon 1180 PASS / mcp 17 PASS / workspace build OK / architecture lisp OK.")

  (claim
    :id wave20-07-claim-001
    :task wave20-07-llm-augmented-plan-inference-v0
    :agent claudecode
    :seq 6
    :at "2026-04-27T03:50:00+08:00"
    :touched ["crates/missiond-daemon/src/handlers/knowledge/plan.rs"
              "crates/missiond-daemon/src/handlers/knowledge/plan_dag.rs"
              "crates/missiond-mcp/src/tools/knowledge/plan.rs"]
    :summary "Add infer_plan_fields=sonnet_suggest mode: deterministic-first, then Sonnet proposals (validated, never auto-applied) with structured llm_status / llm_proposals[] surfacing.")

  (completion
    :id wave20-07-completion-001
    :task wave20-07-llm-augmented-plan-inference-v0
    :agent claudecode
    :seq 7
    :at "2026-04-27T04:10:00+08:00"
    :touched ["crates/missiond-daemon/src/handlers/knowledge/plan.rs"
              "crates/missiond-daemon/src/handlers/knowledge/plan_dag.rs"
              "crates/missiond-mcp/src/tools/knowledge/plan.rs"]
    :summary "wave20-07 done: sonnet_suggest mode plumbed end-to-end. Default off byte-shape preserved; sonnet_suggest is single-node-execute-only (DAG path fails fast via refuse_llm_inference_in_dag_mode). Sonnet unavailability surfaces structured llm_status=llm_unavailable; LLM proposals never auto-apply (applied=false pinned per proposal, conflict_status reconciled against caller args + deterministic high/medium tiers). 1230 daemon tests + 17 mcp tests + workspace build + architecture-lisp + git diff --check all pass.")

  (claim
    :id wave20-04-claim-001
    :task wave20-04-machine-driven-dispatch-v0
    :agent claudecode
    :seq 8
    :at "2026-04-26T15:00:00+08:00"
    :touched ["crates/missiond-daemon/src/handlers/knowledge/plan.rs"
              "crates/missiond-daemon/src/handlers/knowledge/plan_dag.rs"
              "crates/missiond-daemon/src/handlers/knowledge/workstation_dispatch.rs"
              "crates/missiond-daemon/src/handlers/knowledge/unified_entry.rs"
              "crates/missiond-mcp/src/tools/knowledge/plan.rs"]
    :summary "Wire wave-19/07 dormant consumer (run_workstation_dispatch_with_contract) into the active dispatch path via opt-in dispatch_contract_mode=machine / render_markdown=false. Default rendered keeps wave-15..19 byte-compat. Machine mode hands the emitted task.lisp directly to workstation_dispatch (Markdown becomes optional compatibility metadata). Malformed contract continues to return SafeDescriptor; no claude -p fallback. unified_entry forwards both knobs.")

  (completion
    :id wave20-04-completion-001
    :task wave20-04-machine-driven-dispatch-v0
    :agent claudecode
    :seq 9
    :at "2026-04-26T16:00:00+08:00"
    :touched ["crates/missiond-daemon/src/handlers/knowledge/plan.rs"
              "crates/missiond-daemon/src/handlers/knowledge/plan_dag.rs"
              "crates/missiond-daemon/src/handlers/knowledge/workstation_dispatch.rs"
              "crates/missiond-daemon/src/handlers/knowledge/unified_entry.rs"
              "crates/missiond-mcp/src/tools/knowledge/plan.rs"]
    :summary "wave20-04 done at commit 681c95df3eaf: DispatchContractMode {Rendered (default), Machine} parsed from `dispatch_contract_mode` (string allowlist) or `render_markdown` boolean shorthand. Default Rendered preserves wave-15..19 wire shape exactly. Machine mode wires the wave-19/07 dormant consumer: when emit produced a contract path AND dispatch routes through workstation, runner forwards path so consumer reads on-disk task.lisp directly (overlay onto brief, contract is SSOT). Brief now carries `## Source contract` preamble + response surfaces task_contract_source_path (machine + Dispatched only) so observers can prove the Lisp drove the brief. Markdown becomes optional compatibility metadata; render_command still surfaced for out-of-process rendering. Malformed contract → SafeDescriptor (status=`skipped_malformed_task_contract`); no claude -p fallback. DAG path: TaskContractDispatchCtx captures dispatch_contract_mode once at scheduler entry, propagates to every per-node dispatch (no per-node override). unified_entry forwards both wave-20 knobs (`dispatch_contract_mode` / `render_markdown`) AND the previously-unforwarded wave-19 emitter knobs (`task_contract_mode` / `emit_task_contract`) so the unified pipeline drives the full machine-driven loop. 1246 daemon / 17 mcp / 302 core tests + workspace build + check-task-contract --all + check-architecture-lisp --all-v2 + git diff --check + verify-task-contract all pass.")

  (claim
    :id wave20-05-claim-001
    :task wave20-05-unified-entry-machine-loop-smoke-v2
    :agent claudecode
    :seq 10
    :at "2026-04-26T16:30:00+08:00"
    :touched ["crates/missiond-daemon/src/handlers/knowledge/unified_entry.rs"]
    :summary "Add deterministic no-LLM no-spawn smoke proving unified_entry can drive the wave-20/04 machine-driven dispatch loop end-to-end via fixture task.lisp data, with assertions that machine-contract fields survive every handoff, Markdown brief is non-load-bearing in machine mode, and malformed contracts surface a structured SafeDescriptor with no prompt fallback.")

  (completion
    :id wave20-05-completion-001
    :task wave20-05-unified-entry-machine-loop-smoke-v2
    :agent claudecode
    :seq 11
    :at "2026-04-26T17:00:00+08:00"
    :touched ["crates/missiond-daemon/src/handlers/knowledge/unified_entry.rs"]
    :summary "wave20-05 done: extended build_artifact_refs to lift wave-19/06 + wave-20/04 machine-contract keys (task_contract_mode, task_contract_eligible, task_contract_path, task_contract_source_path, dispatch_contract_mode, render_command, task_contract_skip_reason, task_contract_error) so the unified-entry envelope preserves the machine handoff. Added 6 new tests covering: (1) artifact_refs lifts every machine-contract field on a synthesised wave-20/04 inner payload, (2) artifact_refs omits all machine-contract keys when inner handler is silent (wave-15..19 byte-compat preserved), (3) fixture-based contract overlay smoke proves parse_task_contract + WorkstationDispatchHints::overlay_contract + build_task_brief_with_source produce the contract-driven brief without IO, (4) full machine-mode canonical loop smoke (s4 plan-compile dry_run → s6 plan-execute machine mode) asserts task_contract_source_path == task_contract_path SSOT invariant survives the unified-entry decoration, (5) Markdown brief non-load-bearing assertion proves task_brief_preview lives ONLY on the inner payload (not artifact_refs), (6) malformed-contract refusal smoke proves parse_task_contract returns TaskContractParseError::MissingRequired(\"goal\") and the synthesised SafeDescriptor refusal payload preserves workstation_dispatch_status=skipped_malformed_task_contract with no inner_result and no `claude -p` fallback artefact. 1252 daemon (baseline 1246 + 6 new) / 17 mcp / 302 core tests + workspace build + check-task-contract --all + check-architecture-lisp --all-v2 + git diff --check all pass.")

  (claim
    :id wave20-08-claim-001
    :task wave20-08-review-auto-answer-policy-v0
    :agent claudecode
    :seq 12
    :at "2026-04-26T18:00:00+08:00"
    :touched ["crates/missiond-daemon/src/handlers/knowledge/review_gate.rs"
              "crates/missiond-daemon/src/handlers/knowledge/unified_entry.rs"]
    :summary "Add explicit auto_answer_policy (off|deterministic_safe|dry_run; default off) layered on top of wave-18/07 review_automation_policy. Inherits 5 wave-18 safety rules + adds destructive-action / never-auto-reject guards: deterministic_safe MAY auto-answer Approved on review-question listener path; auto-reject is forbidden; archive/supersede/remove actions never promoted. dry_run computes the decision without mutation. Returns policy_result/selected_decision/safety_rule_results/requires_human. unified_entry forwards the new arg through dry-run smoke paths without adding a new MCP tool.")

  (completion
    :id wave20-08-completion-001
    :task wave20-08-review-auto-answer-policy-v0
    :agent claudecode
    :seq 13
    :at "2026-04-26T19:30:00+08:00"
    :touched ["crates/missiond-daemon/src/handlers/knowledge/review_gate.rs"
              "crates/missiond-daemon/src/handlers/knowledge/unified_entry.rs"]
    :summary "wave20-08 done at commit 8adb0a85cda2: AutoAnswerPolicy {Off (default), DeterministicSafe, DryRun} parsed via parse_auto_answer_policy (case-insensitive + trimmed; hyphenated aliases accepted; unknown collapses to Off). evaluate_auto_answer_policy reuses wave-18/07 ReviewAutomationContext + safety inspector and adds two wave-20/08 guards: (1) destructive-action gate (archive|supersede|remove, case-insensitive, centralised in DESTRUCTIVE_REVIEW_ACTIONS) NEVER promotes — pinned as dedicated AutoAnswerStatus::SkippedDestructiveAction so observers can grep the rule trail; (2) caller-decision guard so explicit review_decision always wins. Hard invariants (test-pinned): I1 NEVER auto-reject — selected_decision can never be `rejected` under any mode (the evaluator demotes any upstream Rejected to NeedsChanges and surfaces a `rule:rejection_demoted` audit entry); I2 destructive actions NEVER auto-promote even when every safety rule passes; I3 pure / sync / never calls an LLM (signature enforcement + determinism check); I4 when skipped, response carries policy_result, selected_decision, safety_rule_results[], requires_human=true. dry_run mode evaluates the same inspector + guards but ALWAYS sets requires_human=true. unified_entry forwards `auto_answer_policy` through the plan-execute key list verbatim (pipeline never re-validates — inner handler owns the allowlist) and build_artifact_refs lifts the 5 outcome keys (auto_answer_policy/policy_result/selected_decision/safety_rule_results/requires_human) into the unified envelope so downstream consumers can read the listener-path outcome without re-parsing inner JSON. Default off keeps wave-15..19 byte-shape (the lift only fires when inner stamped a key). NO new MCP tool. NO live LLM auto-approval. NO destructive auto-promotion. unified_entry doc-comment pivot: legacy 'auto-answer a review question' non-goal narrowed to 'auto-answer a review question via LLM' so deterministic listener-side promotion under opt-in deterministic_safe is explicitly documented while preserving the LLM ban. Acceptance: cargo test -p missiond-daemon handlers::knowledge::review_gate::tests → 152 passed (122 baseline + 30 new); cargo test -p missiond-daemon handlers::knowledge::unified_entry::tests → 64 passed (58 baseline + 6 new); cargo test -p missiond-daemon → 1288 passed (baseline 1252 + 36 new); cargo test -p missiond-mcp --lib → 17 passed; cargo build --workspace OK; node scripts/check-architecture-lisp.mjs --all-v2 → 20 v2 lisp files OK; git diff --check on scoped files OK; node scripts/verify-task-contract.mjs OK.")

  (claim
    :id wave20-09-claim-001
    :task wave20-09-execution-event-legacy-metadata-sweep
    :agent claudecode
    :seq 14
    :at "2026-04-26T20:00:00+08:00"
    :touched ["crates/missiond-core/src/event/events/execution.rs"
              "crates/missiond-daemon/src/handlers/knowledge/agent_execution.rs"]
    :summary "Sweep remaining legacy ExecutionEvent variants (Heartbeat / Released / DeviationRecorded / DecisionRecorded / IssueRecorded / Audited / Repaired / StaleClaim) and add optional dispatch_strategy/target_project/requested_cwd serde-default skip-serializing fields. Inherit from companion-log meta block via existing read_dispatch_metadata_from_log helper. Round-trip tests prove backward compatibility (legacy JSON still parses; absent metadata still skip-serializes byte-identical to original wire shape).")

  (completion
    :id wave20-09-completion-001
    :task wave20-09-execution-event-legacy-metadata-sweep
    :agent claudecode
    :seq 15
    :at "2026-04-26T21:00:00+08:00"
    :touched ["crates/missiond-core/src/event/events/execution.rs"
              "crates/missiond-daemon/src/handlers/knowledge/agent_execution.rs"]
    :summary "wave20-09 done at commit 6e01e3fe572f: 8 legacy ExecutionEvent variants (Heartbeat / Released / DeviationRecorded / DecisionRecorded / IssueRecorded / Audited / Repaired / StaleClaim) extended with optional dispatch_strategy/target_project/requested_cwd serde-default skip-serializing fields, matching the projection contract Opened (wave-11) / Claimed+Completed (wave-18/02) / PlanNodeStateChanged (wave-14/02) already carry. Wire-shape impact per variant: Heartbeat 5→8, Released 5→8 (summary stays mandatory key in legacy shape), DeviationRecorded 4→7, DecisionRecorded 4→7, IssueRecorded 4→7, Audited 4→7, Repaired 3→6, StaleClaim 4→7. Inheritance path: every emit site in agent_execution.rs now calls read_dispatch_metadata_from_log(&file) on the post-write LogFile handle and forwards the trio. Audit emits Audited then per-stale-claim StaleClaim sharing one meta snapshot via clone. Serde back-compat triple-pinned per variant: (1) legacy_json_without_dispatch_metadata parses to None; (2) without_dispatch_metadata_serializes_byte_identical_to_legacy asserts exact key set + count; (3) with_dispatch_metadata_round_trips preserves verbatim. Plus a partial-metadata sweep across Heartbeat/Audited/Repaired pins skip-serialize per individual field. Acceptance: cargo test -p missiond-core --lib → 327 passed (302 baseline + 25 new); cargo test -p missiond-daemon handlers::knowledge::agent_execution::tests → 84 passed (68 baseline + 16 new); cargo test -p missiond-daemon → 1304 passed (1288 baseline + 16); cargo test -p missiond-mcp --lib → 17 passed; cargo build --workspace OK; node scripts/check-architecture-lisp.mjs --all-v2 → 20 v2 lisp files OK; git diff --check on scoped files OK; node scripts/verify-task-contract.mjs OK against commit 6e01e3fe572f. Must-not-touch verified clean (plan.rs / plan_dag.rs / workstation_dispatch.rs / .missiond/v2/*.lisp / scripts/**).")

  (claim
    :id wave20-10-claim-001
    :task wave20-10-lisp-backfill-wave20-status
    :agent claudecode
    :seq 16
    :at "2026-04-27T03:30:00+08:00"
    :touched [".missiond/v2/intent-machine-contract.lisp"
              ".missiond/v2/intent-pillar-source-index.lisp"
              ".missiond/v2/intent-flow.lisp"
              ".missiond/v2/intent-intent-layer.lisp"
              ".missiond/v2/intent-tools.lisp"
              ".missiond/v2/intent-plan-dag.lisp"
              ".missiond/v2/intent-workstation-policy.lisp"
              ".missiond/v2/intent-execution-governance.lisp"
              ".missiond/v2/intent.lisp"]
    :summary "Backfill MissionD v2 architecture Lisp after Wave 20 (commits 1fc0fd6/b36cf6c/fe835e8/681c95d/d308fae/3669ebc/6bb935a/8adb0a8/6e01e3f): scoped-commit guard + renderer guard + execution preflight contract scope + machine-driven dispatch + unified-entry machine smoke + cross-plan auto-trigger + LLM-augmented sonnet_suggest + review auto-answer policy + legacy ExecutionEvent metadata sweep — add 9 new source-index anchors + status upgrades. No code, schema, or task-doc edits.")

  (completion
    :id wave20-10-completion-001
    :task wave20-10-lisp-backfill-wave20-status
    :agent claudecode
    :seq 17
    :at "2026-04-27T04:30:00+08:00"
    :touched [".missiond/v2/intent-machine-contract.lisp"
              ".missiond/v2/intent-pillar-source-index.lisp"
              ".missiond/v2/intent-flow.lisp"
              ".missiond/v2/intent-intent-layer.lisp"
              ".missiond/v2/intent-tools.lisp"
              ".missiond/v2/intent-plan-dag.lisp"
              ".missiond/v2/intent-workstation-policy.lisp"
              ".missiond/v2/intent-execution-governance.lisp"
              ".missiond/v2/intent.lisp"]
    :summary "wave20-10 done at commit b94cb7b4ba0e: 9 new source-index anchors under (wave-20-backfill v1.1) covering all wave-20 task 01-09 implementation commits + 3 status-upgrade entries (machine-contract task-contract-v1 note ext machine-mode SSOT; review-automation-policy-v0 partial→full with auto-answer-policy v0; ExecutionEvent dispatch-metadata-v1 note ext 11 variants closure). Header status strings + judgement-now block updated: deferred-coverage :reason gains v1.1; pre-compression-checklist count line gains wave 19 task 12 + wave 20 task 10; new :wave20-task-10-non-goal block; new :wave-20-status-summary; :next-step rewritten to lead with wave-20 closures. intent-machine-contract.lisp pillar :version v0.2→v0.3 + non-goals-v0 narrowed (Markdown still ergonomic but no longer load-bearing post-wave-20/04) + next-steps appended with 9 DONE wave-20 entries. intent.lisp source-of-truth-index :desc bumped to v1.1 (~136 entries) + 7 new navigation-asset entries (machine-contract-task-protocol-v1 / machine-driven-dispatch-v0 / task-scope-index-guard-v1 / renderer-scoped-commit-guard-v2 / execution-preflight-task-contract-scope-v1 / review-auto-answer-policy-v0 / execution-event-dispatch-metadata-full-v0) + pillar memory/worker/flow :status strings extended. intent-flow.lisp / intent-intent-layer.lisp / intent-tools.lisp / intent-plan-dag.lisp / intent-workstation-policy.lisp / intent-execution-governance.lisp top-level :status / :canonical-status / shard :status strings each extended with the wave-20 closures relevant to that pillar / shard. Acceptance: node scripts/check-architecture-lisp.mjs --all-v2 → 20 v2 lisp files OK; node scripts/check-task-contract.mjs --all → 26 tasks OK; git diff --check on 9 scoped files OK; node scripts/check-task-memory.mjs → 1 ledger 16 entries OK (now 17 with this completion entry); node scripts/check-task-report.mjs → 1 report OK; node scripts/verify-task-contract.mjs → task-contract verify OK against b94cb7b4ba0e (5 checks: commit hash exists / commit message contains task-id / changed_files ⊆ write-scope / changed_files ∩ must-not-touch = ∅ / acceptance commands present). Constraints honored: no Rust/SQL/JS/Cargo/task-doc edits; no event-bus.lisp / intent-mcp-defs.lisp touched; no anchor deleted / no shard split; section-id stability R008 + R016 preserved (3 status-upgrade entries explicitly reuse existing section-ids). Honest pending list captured: full LLM auto-approve (4 v0 non-goals) / autonomous workstation spawn no-hint / git config core.hooksPath .githooks default-on / sonnet fully autonomous distill chain / LLM-augmented PLAN inference apply (sonnet_suggest only suggests) / frontend Lisp."))
