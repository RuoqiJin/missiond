;; Wave 21 shared-memory ledger.
;; Schema: .missiond/tasks/schema/shared-memory-v1.lisp
;; Checker: scripts/check-task-memory.mjs
;;
;; Append-only. Agents add entries while they hold a live :claim for their
;; task id. Editing or removing prior entries is forbidden; append a
;; (correction ...) entry instead.

(shared-memory wave21
  :schema "missiond.shared-memory.v1"
  :wave wave21
  :created-at "2026-04-27T00:00:00+08:00"
  :sequence 1

  (observation
    :id wave21-bootstrap-001
    :task wave21-00-archive-wave20-task-artifacts
    :agent codex-orchestrator
    :seq 1
    :at "2026-04-27T00:00:00+08:00"
    :touched []
    :summary "Bootstrap entry: Wave 21 focuses on default-on scope guardrails, run verification, and explicit LLM/autonomous proposal-to-apply gates.")

  (claim
    :id wave21-00-claim-001
    :task wave21-00-archive-wave20-task-artifacts
    :agent claudecode
    :seq 2
    :at "2026-04-26T12:00:00+08:00"
    :touched [".missiond/tasks/wave20/wave20-01-task-scope-index-guard-v1.lisp"
              ".missiond/tasks/wave20/wave20-02-renderer-scoped-commit-guard-v2.lisp"
              ".missiond/tasks/wave20/wave20-03-execution-preflight-contract-scope-v1.lisp"
              ".missiond/tasks/wave20/wave20-04-machine-driven-dispatch-v0.lisp"
              ".missiond/tasks/wave20/wave20-05-unified-entry-machine-loop-smoke-v2.lisp"
              ".missiond/tasks/wave20/wave20-06-cross-plan-distill-auto-trigger-v1.lisp"
              ".missiond/tasks/wave20/wave20-07-llm-augmented-plan-inference-v0.lisp"
              ".missiond/tasks/wave20/wave20-08-review-auto-answer-policy-v0.lisp"
              ".missiond/tasks/wave20/wave20-09-execution-event-legacy-metadata-sweep.lisp"
              ".missiond/tasks/wave20/wave20-10-lisp-backfill-wave20-status.lisp"
              ".missiond/tasks/wave20/wave20-11-parallel-dispatch-index.lisp"
              ".missiond/tasks/wave20/shared-memory.lisp"
              ".missiond/tasks/wave20/reports/wave20-03-execution-preflight-contract-scope-v1.report.lisp"
              ".missiond/tasks/wave20/reports/wave20-04-machine-driven-dispatch-v0.report.lisp"
              ".missiond/tasks/wave20/reports/wave20-05-unified-entry-machine-loop-smoke-v2.report.lisp"
              ".missiond/tasks/wave20/reports/wave20-06-cross-plan-distill-auto-trigger-v1.report.lisp"
              ".missiond/tasks/wave20/reports/wave20-07-llm-augmented-plan-inference-v0.report.lisp"
              ".missiond/tasks/wave20/reports/wave20-08-review-auto-answer-policy-v0.report.lisp"
              ".missiond/tasks/wave20/reports/wave20-09-execution-event-legacy-metadata-sweep.report.lisp"
              ".missiond/tasks/wave20/reports/wave20-10-lisp-backfill-wave20-status.report.lisp"
              ".missiond/claudecode/wave20-01-task-scope-index-guard-v1.md"
              ".missiond/claudecode/wave20-02-renderer-scoped-commit-guard-v2.md"
              ".missiond/claudecode/wave20-03-execution-preflight-contract-scope-v1.md"
              ".missiond/claudecode/wave20-04-machine-driven-dispatch-v0.md"
              ".missiond/claudecode/wave20-05-unified-entry-machine-loop-smoke-v2.md"
              ".missiond/claudecode/wave20-06-cross-plan-distill-auto-trigger-v1.md"
              ".missiond/claudecode/wave20-07-llm-augmented-plan-inference-v0.md"
              ".missiond/claudecode/wave20-08-review-auto-answer-policy-v0.md"
              ".missiond/claudecode/wave20-09-execution-event-legacy-metadata-sweep.md"
              ".missiond/claudecode/wave20-10-lisp-backfill-wave20-status.md"
              ".missiond/claudecode/wave20-11-parallel-dispatch-index.md"]
    :summary "Stage and commit Wave 20 task artifacts (11 contracts + 11 briefs + 8 reports + 1 shared-memory ledger) under chore(wave20) per task-scope-guard staged mode.")

  (completion
    :id wave21-00-completion-001
    :task wave21-00-archive-wave20-task-artifacts
    :agent claudecode
    :seq 3
    :at "2026-04-26T12:30:00+08:00"
    :touched [".missiond/tasks/wave21/reports/wave21-00-archive-wave20-task-artifacts.report.lisp"
              ".missiond/tasks/wave21/shared-memory.lisp"]
    :summary "Committed 31 wave20 artifacts (commit 6a7d4e10d5f1: 11 contracts + 11 briefs + 8 reports + 1 shared-memory ledger). All 5 acceptance commands green: check-task-contract.mjs --all (37 tasks); check-task-memory.mjs wave20 (1 ledger, 17 entries); git diff --check (no whitespace); task-scope-guard --mode staged (31 staged files all in :write-scope); verify-task-contract.mjs (commit message + scope + must-not-touch + acceptance presence all OK against 6a7d4e10d5f1). Report written to .missiond/tasks/wave21/reports/wave21-00-archive-wave20-task-artifacts.report.lisp (out-of-scope per contract, intentionally untracked).")

  (claim
    :id wave21-01-claim-001
    :task wave21-01-hooks-path-installer-v1
    :agent claudecode
    :seq 4
    :at "2026-04-26T13:00:00+08:00"
    :touched ["scripts/install-missiond-hooks.mjs"
              "scripts/check-missiond-hooks.mjs"
              ".githooks/pre-commit"
              ".missiond/tasks/schema/task-contract-v1.lisp"]
    :summary "Begin wave21-01: explicit installer + doctor for repo-local core.hooksPath .githooks. install-missiond-hooks.mjs adds --check/--install/--json/--dry-fixture; check-missiond-hooks.mjs is the read-only doctor alias; .githooks/pre-commit stays MISSIOND_TASK_CONTRACT-gated and delegates to task-scope-guard; schema records the installer contract.")

  (claim
    :id wave21-04-claim-001
    :task wave21-04-autonomous-workstation-llm-proposal-v0
    :agent claudecode
    :seq 5
    :at "2026-04-26T22:40:00+08:00"
    :touched ["crates/missiond-daemon/src/handlers/knowledge/workstation_dispatch.rs"
              "crates/missiond-daemon/src/handlers/knowledge/plan.rs"
              "crates/missiond-mcp/src/tools/knowledge/plan.rs"]
    :summary "Begin wave21-04: opt-in `workstation_inference_mode=\"sonnet_suggest\"` that asks Sonnet to PROPOSE workstation dispatch fields (target/dispatch_strategy/objective/scope) ONLY when PLAN hints + caller args are absent (no PLAN.lisp opt-in, no auto-inference fired). Proposals validated into structured records (field, value, confidence, evidence, safety_status); surfaced under `workstation_proposals[]`; NEVER auto-spawn / auto-apply. Sonnet unavailable → LLM_UNAVAILABLE bundle, no fallback to claude -p. DAG mode rejects sonnet_suggest. Default mode `off` preserves wave-15..20 byte-shape.")

  (completion
    :id wave21-01-completion-001
    :task wave21-01-hooks-path-installer-v1
    :agent claudecode
    :seq 6
    :at "2026-04-26T13:30:00+08:00"
    :touched ["scripts/install-missiond-hooks.mjs"
              "scripts/check-missiond-hooks.mjs"
              ".githooks/pre-commit"
              ".missiond/tasks/schema/task-contract-v1.lisp"]
    :summary "Committed wave21-01 (commit 44c74df40182, 4 files / +593 / -5). install-missiond-hooks.mjs ships --check/--install/--json/--dry-fixture/--strict; check-missiond-hooks.mjs is the read-only doctor alias (rejects --install/--dry-fixture). Installer is repo-local only: --install runs `git config --local core.hooksPath .githooks` exactly once and is no-op when already aligned; never touches --global/--system. All 5 acceptance commands green: dry-fixture (8/8); --check --json (drift detected as expected since this clone is not yet opted in: current=.git/hooks, expected=.githooks); check-task-contract.mjs --all (37 tasks); check-architecture-lisp.mjs --all-v2 (20 files); git diff --check (no whitespace). Default behaviour stays opt-in (per task brief: 'Do not enable hooks globally; repo-local only'); enable per clone via `node scripts/install-missiond-hooks.mjs --install`. Schema records hooks-installer-contract section. Report written out-of-scope to .missiond/tasks/wave21/reports/wave21-01-hooks-path-installer-v1.report.lisp; intentionally untracked per wave21 protocol.")

  (claim
    :id wave21-02-claim-001
    :task wave21-02-run-verifier-v1
    :agent claudecode
    :seq 7
    :at "2026-04-26T14:00:00+08:00"
    :touched ["scripts/verify-task-run.mjs"
              "scripts/verify-task-contract.mjs"
              ".missiond/tasks/schema/report-contract-v1.lisp"
              ".missiond/tasks/schema/shared-memory-v1.lisp"]
    :summary "Begin wave21-02: scripts/verify-task-run.mjs as the three-in-one (task contract + report + shared-memory + commit) post-run proof. Reuses verify-task-contract.mjs core via newly gated main(); cross-links both schema files to the new run-verifier. Read-only by construction; --allow-missing-memory required to skip the ledger check explicitly.")

  (completion
    :id wave21-02-completion-001
    :task wave21-02-run-verifier-v1
    :agent claudecode
    :seq 8
    :at "2026-04-26T14:30:00+08:00"
    :touched ["scripts/verify-task-run.mjs"
              "scripts/verify-task-contract.mjs"
              ".missiond/tasks/schema/report-contract-v1.lisp"
              ".missiond/tasks/schema/shared-memory-v1.lisp"]
    :summary "Committed wave21-02 (commit 1335fa7fbe85, 4 files / +895 / -3). scripts/verify-task-run.mjs is the three-in-one (task contract + report + shared-memory + commit) post-run proof: --task/--report/--memory/--commit/--json/--dry-fixture/--allow-missing-memory; delegates contract verification to verify-task-contract.verifyContract; checks report :task_id == contract id, report :commit_hash matches resolved git sha (full or >=7-hex prefix), and shared-memory has a (completion :task <id>) entry. Missing --memory or non-existent ledger fails with structured diagnostic unless --allow-missing-memory is passed. Read-only by construction (assertReadOnlyGit allowlist documents the 14 forbidden git verbs; only verify-task-contract.readCommit's `git rev-parse | log | show` are reachable). 12 dry-fixtures + 7 helper cases all green. verify-task-contract.mjs main() now gated by import.meta.url so importers don't trigger CLI parsing. Schema files (report-contract-v1.lisp, shared-memory-v1.lisp) cross-link the new :run-verifier. All 6 acceptance commands green: verify-task-run --dry-fixture (12+7); check-task-contract --all (37 tasks); check-task-report --dry-fixture (10); check-task-memory --dry-fixture (13); check-architecture-lisp --all-v2 (20 files); git diff --check (no whitespace). Pre-commit task-scope-guard --mode staged: 4 staged files, all in :write-scope. Post-commit verify-task-contract: 1335fa7fbe85 against contract OK. Self-verify (dogfood) verify-task-run: ok=true, all 4 checks green against the new commit. Report written out-of-scope to .missiond/tasks/wave21/reports/wave21-02-run-verifier-v1.report.lisp; intentionally untracked per wave21 protocol.")

  (claim
    :id wave21-03-claim-001
    :task wave21-03-execution-report-verifier-integration-v1
    :agent claudecode
    :seq 9
    :at "2026-04-26T15:00:00+08:00"
    :touched ["crates/missiond-daemon/src/handlers/knowledge/agent_execution.rs"
              "crates/missiond-mcp/src/tools/knowledge/agent_execution.rs"]
    :summary "Begin wave21-03: expose task-run verification status through mission_execution complete/preflight without daemon-side mutating git. action=complete grows optional task_run_verifier_status (enum), shared_memory_path, verifier_diagnostics (free-form), and verified (bool). verified=true triggers a daemon read-only cross-check that loads the report file off disk and asserts :schema=missiond.report-contract.v1, :task_id matches contract head id, :commit_hash matches the supplied hash. Preconditions (enforce_scoped_commit + task_contract_path + task_report_path + commit_hash) all rejected fail-fast with VERIFIED_REQUIRES_* before any companion log mutation. Daemon NEVER spawns Node; the wave21-02 scripts/verify-task-run.mjs remains the out-of-process authority. preflight_commit also echoes task_report_path/shared_memory_path advisory hints. Legacy callers omitting all wave21 fields keep byte-identical companion log shape.")

  (completion
    :id wave21-04-completion-001
    :task wave21-04-autonomous-workstation-llm-proposal-v0
    :agent claudecode
    :seq 10
    :at "2026-04-26T23:10:00+08:00"
    :touched ["crates/missiond-daemon/src/handlers/knowledge/workstation_dispatch.rs"
              "crates/missiond-daemon/src/handlers/knowledge/plan.rs"
              "crates/missiond-mcp/src/tools/knowledge/plan.rs"
              ".missiond/tasks/wave21/reports/wave21-04-autonomous-workstation-llm-proposal-v0.report.lisp"
              ".missiond/tasks/wave21/shared-memory.lisp"]
    :summary "Committed wave21-04 (commit 68b84f123b5e, 3 files / +1704 / -0). New `workstation_inference_mode` knob (off | sonnet_suggest, default off preserves wave-15..20 byte-shape). When opted in AND caller / PLAN supplied no signals (gated by WorkstationProposalGate: caller_target/dispatch_strategy/objective/scope/owned_files/project_signal AND plan_hints + plan_workstation_opt_in all false), the runner asks Sonnet to PROPOSE values for the four core workstation knobs (target / dispatch_strategy / objective / scope). Proposals validated into structured records (field, value, confidence, evidence, safety_status); cap 6; surfaced under `workstation_proposals[]` with bundle-level auto_spawn=false and per-proposal applied=false; NEVER auto-spawn / auto-apply. Conservative whitelists: target ∈ {mission_execution, mission_task_delegate, mission_flow_run}; dispatch_strategy ∈ {resident-lisp, fresh-code-alignment, agent-team, mixed} (prompt-fallback / unknown excluded). Sonnet unavailable surfaces status=\"llm_unavailable\" + unavailable_reason text pinning \"no fallback to claude -p / prompt mode in v0\"; gate-suppressed surfaces status=\"plan_hints_present\" + signal_summary listing which slots fired. DAG mode (scheduler_mode=dag_v1) preflight rejects sonnet_suggest with INVALID_PARAM. attach_workstation_proposals_block splices the bundle onto every dispatch branch (executing / dispatch_skipped / dry_run / inner_error / safe-descriptor / bridge); errors and pre-existing keys preserved untouched. Acceptance: cargo test workstation_dispatch::tests (109 passed) / plan::tests (249 passed) / cargo build --workspace OK / cargo test missiond-mcp --lib (17/17) / check-architecture-lisp --all-v2 (20 files) / git diff --check (no whitespace). cargo test missiond-core (327 baseline passed). cargo test -p missiond-daemon: 1369 passed; 1 failed (handlers::knowledge::agent_execution::tests::daemon_never_invokes_mutating_git — wave21-03 introduced test that does std::fs::read_to_string(file!()) which fails when CWD != workspace root; this is wave21-03's bug, NOT introduced by wave21-04). Pre-commit task-scope-guard --mode staged: 3 staged files all in :write-scope. Post-commit verify-task-contract: 68b84f123b5e against contract OK. Report written out-of-scope to .missiond/tasks/wave21/reports/wave21-04-autonomous-workstation-llm-proposal-v0.report.lisp (intentionally untracked per wave21 protocol).")

  (completion
    :id wave21-03-completion-001
    :task wave21-03-execution-report-verifier-integration-v1
    :agent claudecode
    :seq 11
    :at "2026-04-26T15:30:00+08:00"
    :touched ["crates/missiond-daemon/src/handlers/knowledge/agent_execution.rs"
              "crates/missiond-mcp/src/tools/knowledge/agent_execution.rs"]
    :summary "Committed wave21-03 (commit 308426e15c11, 2 files / +1083 / -0). action=complete grows four optional task-run verifier slots: task_run_verifier_status (enum passed|failed|skipped|unknown), shared_memory_path (string), verifier_diagnostics (free-form), and verified (bool). verified=true lights up daemon-side enforce_verified_completion gate which (a) requires preconditions (enforce_scoped_commit=true + task_contract_path + task_report_path + commit_hash), (b) loads the report file off disk via local sexp module (no new dependency), (c) asserts :schema=missiond.report-contract.v1, :task_id matches contract head id, :commit_hash matches supplied hash with short<->long sha prefix overlap. Failures: VERIFIED_REQUIRES_ENFORCEMENT/TASK_CONTRACT/TASK_REPORT/COMMIT_HASH, TASK_REPORT_REQUIRED/MALFORMED, TASK_REPORT_TASK_ID_MISMATCH, TASK_REPORT_COMMIT_HASH_MISMATCH — all rejected BEFORE any companion log mutation. Persistence: each new field writes verbatim into the completion entry as a Lisp keyword (`:task-run-verifier-status`, `:shared-memory-path`, `:verifier-diagnostics`, `:verified`); `verified` writes as bare true/false atom. CompletionRecord struct extended with four new Option fields; legacy companion logs omitting them keep byte-identical shape. Response surface mirrors fields when supplied + emits verified_validation summary on happy path. action=preflight_commit also echoes task_report_path/shared_memory_path advisory hints. MCP schema (crates/missiond-mcp/src/tools/knowledge/agent_execution.rs) gains four new properties + tool description appended with wave21-03 paragraph. daemon_never_invokes_mutating_git test fixed to anchor at CARGO_MANIFEST_DIR and to build the search needle at runtime (avoids self-counting); `Command::new(<git>)` over agent_execution.rs returns ONE hit (wave18-08 status read); zero mutating git argv strings introduced. All 6 acceptance commands green: cargo test -p missiond-daemon agent_execution::tests (102/102, 17 new); cargo test -p missiond-daemon (1370/1370); cargo test -p missiond-mcp --lib (17/17); cargo build --workspace clean; check-architecture-lisp --all-v2 (20 files); git diff --check (no whitespace). Pre-commit task-scope-guard --mode staged: 2 staged files, both in :write-scope. Post-commit verify-task-contract: 308426e15c11 against contract OK. Report written out-of-scope to .missiond/tasks/wave21/reports/wave21-03-execution-report-verifier-integration-v1.report.lisp; intentionally untracked per wave21 protocol.")

  (claim
    :id wave21-05-claim-001
    :task wave21-05-plan-inference-apply-gate-v1
    :agent claudecode
    :seq 12
    :at "2026-04-26T23:30:00+08:00"
    :touched ["crates/missiond-daemon/src/handlers/knowledge/plan.rs"
              "crates/missiond-mcp/src/tools/knowledge/plan.rs"]
    :summary "Begin wave21-05: opt-in `apply_inferred_fields=true` apply gate v1 layered on top of wave-18/06 deterministic + wave-20/07 LLM inference. Default suggest-only (preserves wave-18 byte-shape). When opted in, deterministic high-confidence + no-conflict fields promoted into apply_gate.applied_fields[]; LLM proposals require explicit `llm_caller_approved` (object/array shape) AND confidence ∈ {high, medium} AND conflict_status=none AND per-field safety check (mirrors wave-21/04 whitelists). Caller-vs-inferred conflicts NEVER apply (surface on conflict_fields[]). Persist boundary v1: persisted plan.sexp_text NEVER mutated; persist_inference_applied hard-pinned to false; persist_inference flag echoed to audit. Strict shape validation: only bool form accepted for apply_inferred_fields/persist_inference; only object/array for llm_caller_approved; typo string `\"true\"` fail-fast INVALID_PARAM.")

  (completion
    :id wave21-05-completion-001
    :task wave21-05-plan-inference-apply-gate-v1
    :agent claudecode
    :seq 13
    :at "2026-04-26T23:55:00+08:00"
    :touched ["crates/missiond-daemon/src/handlers/knowledge/plan.rs"
              "crates/missiond-mcp/src/tools/knowledge/plan.rs"]
    :summary "Committed wave21-05 (commit a18200b007a1, 2 files / +1304 / -77). New apply gate v1: `apply_inferred_fields` (bool, default false) + `llm_caller_approved` (object {field: bool} or array of field strings) + `persist_inference` (bool, default false) — all strict-shape validated pre-flight (string \"true\" rejected with INVALID_PARAM). Default suggest-only ⇒ wave-18..20 byte-shape preserved (wave-18 ApplySafe still auto-fills high-confidence slots when gate flag absent for back-compat). Opt-in (apply_inferred_fields=true) promotes ONLY: (a) deterministic high-confidence + no-conflict + caller-slot-empty fields; (b) wave-20 LLM proposals where caller named the field in llm_caller_approved AND confidence ∈ {high, medium} AND conflict_status=none AND per-field safety check passes (target ∈ {mission_execution, mission_task_delegate, mission_flow_run}; dispatch_strategy ∈ {resident-lisp, fresh-code-alignment, agent-team, mixed} — prompt-fallback/unknown excluded; acceptance_mode via canonicalize_acceptance_mode; owned_files non-empty string array; target_project non-empty; workstation_dispatch bool). Caller-vs-inferred conflicts NEVER apply (land on conflict_fields[]). Sub-threshold suggestions never apply (skipped reason `below_apply_threshold`). Response surfaces `apply_gate` block on every dispatch branch (executing / dispatch_skipped / dry_run / inner_error / safe-descriptor / bridge / preview short-circuit / sonnet_suggest short-circuit) with stable shape: {requested, persist_inference_requested, persist_inference_applied (always false in v1 — hard-pinned invariant), applied_fields[] (each {field, value, source, origin ∈ {deterministic_inferred, llm_proposal}}), skipped_fields[] (each {field, reason ∈ {apply_gate_not_requested, caller_value_already_set, caller_value_conflict, below_apply_threshold, llm_not_caller_approved, llm_confidence_too_low, llm_conflict_present, llm_safety_check_failed, deterministic_inferred_already_applied}, origin, [detail]}), conflict_fields[], resulting_plan_preview (caller args ∪ applied)}. Persistence boundary v1: persisted plan.sexp_text NEVER mutated; persist_inference flag echoed for future-wave wiring (a future wave will land the persisted plan write under existing `persist=true` action arg or this explicit flag). attach_apply_gate_block mirrors attach_inference_block / attach_workstation_proposals_block: errors / pre-existing keys preserved untouched. 22 new unit tests added (all pure / no AppState). All 7 acceptance commands green: cargo test plan_dag::tests (249/249) / plan::tests (271/271, 22 new) / cargo test -p missiond-daemon (1392/1392) / cargo test -p missiond-mcp --lib (17/17) / cargo build --workspace clean / check-architecture-lisp --all-v2 (20 files) / git diff --check (no whitespace). Pre-commit task-scope-guard --mode staged: 2 staged files all in :write-scope. Post-commit verify-task-contract: a18200b007a1 against contract OK. plan_dag.rs unchanged (in scope but no DAG-side wiring needed — DAG path already rejects sonnet_suggest at preflight, and the wave-18 ApplySafe back-compat is preserved for the dag_v1 inference branch). Report written out-of-scope to .missiond/tasks/wave21/reports/wave21-05-plan-inference-apply-gate-v1.report.lisp; intentionally untracked per wave21 protocol.")

  (claim
    :id wave21-06-claim-001
    :task wave21-06-llm-auto-approve-proposal-v0
    :agent claudecode
    :seq 14
    :at "2026-04-27T00:10:00+08:00"
    :touched ["crates/missiond-daemon/src/handlers/knowledge/review_gate.rs"
              "crates/missiond-daemon/src/handlers/knowledge/directive.rs"
              "crates/missiond-daemon/src/handlers/knowledge/plan.rs"
              "crates/missiond-mcp/src/tools/knowledge/directive.rs"
              "crates/missiond-mcp/src/tools/knowledge/plan.rs"]
    :summary "Begin wave21-06: opt-in `auto_approve_mode=\"sonnet_suggest\"` propose-only Sonnet pass for directive (approve|archive) + plan (approve|mark|supersede) review surfaces. ORTHOGONAL to wave-18/07 review_automation_policy AND wave-20/08 auto_answer_policy — three knobs co-exist on response. Default `off` preserves wave-18..20 byte-shape. v0 propose-only: applied=false / requires_human=true PINNED on every proposal regardless of confidence (invariant I3). decision=rejected from model demoted to needs_changes (invariant I1, never auto-reject). Destructive actions (archive | supersede | remove) ALWAYS short-circuit to destructive_blocked without LLM call (invariant I2). Sonnet unavailable → llm_unavailable, no fallback to deterministic (invariant I4). destructive_check ALWAYS sourced from is_destructive_review_action, never model (invariant I5).")

  (completion
    :id wave21-06-completion-001
    :task wave21-06-llm-auto-approve-proposal-v0
    :agent claudecode
    :seq 15
    :at "2026-04-27T00:35:00+08:00"
    :touched ["crates/missiond-daemon/src/handlers/knowledge/review_gate.rs"
              "crates/missiond-daemon/src/handlers/knowledge/directive.rs"
              "crates/missiond-daemon/src/handlers/knowledge/plan.rs"
              "crates/missiond-mcp/src/tools/knowledge/directive.rs"
              "crates/missiond-mcp/src/tools/knowledge/plan.rs"
              ".missiond/tasks/wave21/reports/wave21-06-llm-auto-approve-proposal-v0.report.lisp"
              ".missiond/tasks/wave21/shared-memory.lisp"]
    :summary "Committed wave21-06 (commit e14077383b09, 5 files / +1960 / -10). New `auto_approve_mode` knob (off | sonnet_suggest, default off preserves wave-18..20 byte-shape). When opted in, asks Sonnet to PROPOSE a structured review decision and surfaces it under `llm_auto_approve_proposal*` on every dispatch branch (legacy / wave-15 explicit / wave-18 policy-only). Bundle shape: {mode, status (not_invoked|llm_unavailable|suggested|destructive_blocked|no_suggestion), proposal {decision, confidence, evidence, non_goal_check, destructive_check, requires_human, applied=false}, proposal_warnings[], unavailable_reason, action, request_caller, model}. Wired into directive.action_approve / action_archive / both _with_resolution / both _with_policy_only AND plan.action_approve / action_mark / action_supersede + their _with_resolution + _with_policy_only sister functions (10 total branch sites). Strict invariants pinned by 22 new pure unit tests (review_gate.rs:tests::auto_approve_*): (I1) proposal NEVER carries decision=rejected; rejected from model demoted to needs_changes with proposal_warnings[] entry. (I2) destructive actions (archive | supersede | remove, case-insensitive) short-circuit to destructive_blocked WITHOUT calling Sonnet — proposal=None, requires_human=true pinned. (I3) applied=false pinned on EVERY proposal; requires_human=true forced even on non-destructive in v0 (propose-only). (I4) Sonnet unavailable surfaces status=llm_unavailable + unavailable_reason; NO fallback to deterministic. (I5) destructive_check ALWAYS sourced from is_destructive_review_action(action), overwriting model output via enforce_proposal_invariants helper. Strict-enum parse: typo `auto_approve_mode=\"auto\"` fails fast with INVALID_PARAM (parse_proposer_mode_or_error) BEFORE any DB read. Caller-supplied `review_decision` ALWAYS wins (the proposal NEVER overrides explicit human decisions or the deterministic policy outcome). MCP descriptors updated on directive.rs + plan.rs with full invariant table + paragraph in tool-level Chinese description. All 8 acceptance commands green: cargo test review_gate::tests (180/180, 22 new) / directive::tests (43/43) / plan::tests (271/271) / cargo test -p missiond-daemon (1420/1420, +28 from 1392 baseline) / cargo test -p missiond-mcp --lib (17/17) / cargo build --workspace clean / check-architecture-lisp --all-v2 (20 files) / git diff --check (no whitespace). cargo test -p missiond-core baseline 327/327 (orthogonal — not touched). Pre-commit task-scope-guard --mode staged: 5 staged files, all in :write-scope. Post-commit verify-task-contract: e14077383b09 against contract OK (commit message `feat(review): propose LLM auto approvals` matches; scope check write-scope-only OK; must-not-touch clean). Report written out-of-scope to .missiond/tasks/wave21/reports/wave21-06-llm-auto-approve-proposal-v0.report.lisp; intentionally untracked per wave21 protocol. workflow.rs (wave21-07 scope) deliberately untouched — review-resolve workflow surface stays for the next task.")

  (claim
    :id wave21-07-claim-001
    :task wave21-07-sonnet-distill-chain-auto-apply-v1
    :agent claudecode
    :seq 16
    :at "2026-04-27T00:50:00+08:00"
    :touched ["crates/missiond-daemon/src/handlers/knowledge/workflow.rs"
              "crates/missiond-daemon/src/handlers/knowledge/plan.rs"
              "crates/missiond-mcp/src/tools/knowledge/workflow.rs"]
    :summary "Begin wave21-07: opt-in `auto_sonnet=true` apply-gate v1 layered on wave-20/06 cross-plan distill auto-trigger. Default off → wave-20 byte-shape preserved. When opted in alongside `auto_chain_trigger=auto_safe`, ALL 6 wave-20 deterministic safety rules pass, AND caller explicitly attests `auto_sonnet_approved=true`, AND caller's `distill_mode` is NOT already `sonnet`, the daemon auto-promotes the inner distill from dry_run to sonnet (mission_workflow.action_distill_sonnet direct-call) and replaces the outer payload with the sonnet payload (carrying forward wave-19 auto_chain + wave-20 auto_trigger receipts). Conservative invariants: I1 default-off byte-shape preserved; I2 BOTH `auto_sonnet=true` AND `auto_sonnet_approved=true` required (single typo cannot escalate); I3 NEVER without all 6 wave-20 rules passing (gate REUSES trigger outcomes — never relaxes); I4 NEVER when caller's distill_mode already sonnet (no double-call); I5 on Sonnet failure (model error/invalid output) PRESERVE inner payload + surface model_call_status=failed|invalid_output; I6 review_required=true PINNED (auto-applied sonnet stays receipt-only); I7 wave-19/20 blocks remain UNCHANGED (auto_sonnet purely additive). Strict shape: typo `auto_sonnet=\"true\"` (string) / `auto_sonnet_approved=1` (number) → INVALID_PARAM fail-fast at action entry (workflow.rs::validate_auto_sonnet_args + plan.rs::validate_distill_chain_args defense-in-depth). plan.rs distill_chain forwarder propagates auto_sonnet/auto_sonnet_approved/auto_chain_trigger/auto_trigger_min_evidence into the workflow.distill sub-call so plan-side callers can opt into the gate without re-shaping the arg envelope.")

  (completion
    :id wave21-07-completion-001
    :task wave21-07-sonnet-distill-chain-auto-apply-v1
    :agent claudecode
    :seq 17
    :at "2026-04-27T01:25:00+08:00"
    :touched ["crates/missiond-daemon/src/handlers/knowledge/workflow.rs"
              "crates/missiond-daemon/src/handlers/knowledge/plan.rs"
              "crates/missiond-mcp/src/tools/knowledge/workflow.rs"
              ".missiond/tasks/wave21/reports/wave21-07-sonnet-distill-chain-auto-apply-v1.report.lisp"
              ".missiond/tasks/wave21/shared-memory.lisp"]
    :summary "Committed wave21-07 (commit 4d494dbd87d5). New `auto_sonnet` apply-gate v1 (default false → wave-20 byte-shape preserved). When opted in alongside wave-20 `auto_chain_trigger=auto_safe` AND ALL 6 deterministic safety rules pass AND caller attests `auto_sonnet_approved=true` AND `distill_mode` is not already sonnet, the daemon auto-promotes the inner distill from dry_run to sonnet via direct call to action_distill_sonnet and replaces the outer payload (carrying forward wave-19 auto_chain + wave-20 auto_trigger receipts). 8-status taxonomy: not_requested | disabled | skipped_no_trigger | skipped_rules_failed | skipped_caller_approval_missing | skipped_already_sonnet | skipped_inner_error | applied_sonnet. 4-status model_call_status taxonomy: not_invoked | invoked | failed | invalid_output. Block shape: {requested, status, applied, review_required, model_call_status, safety_rule_results, caller_approval, caller_distill_mode?, model_call_error?, sidecar?, chain_id?} + top-level shortcut auto_sonnet_status. Conservative invariants I1-I7 enforced: I1 default-off; I2 dual opt-in required (single typo cannot escalate); I3 ALL 6 wave-20 rules must pass (gate REUSES trigger outcomes); I4 caller-already-sonnet refusal; I5 Sonnet failure PRESERVES inner payload (handler error → status=applied_sonnet + model_call_status=failed; structured error envelope from sonnet → model_call_status=invalid_output); I6 review_required=true PINNED on every successful auto-apply (receipt-only, no DB transition); I7 wave-19/20 blocks UNCHANGED (purely additive). Strict shape validation: workflow.rs::validate_auto_sonnet_args (front-line at action entry) + plan.rs::validate_distill_chain_args defense-in-depth — typo `auto_sonnet=\"true\"` (string) / `auto_sonnet_approved=1` (number) fails fast INVALID_PARAM with shape-label diagnostic. plan.rs::apply_distill_chain forwards auto_sonnet/auto_sonnet_approved/auto_chain_trigger/auto_trigger_min_evidence into workflow.distill sub-call so plan-side callers reach the same gate. MCP descriptor on workflow.rs gains `auto_sonnet` + `auto_sonnet_approved` properties with full invariant table + paragraph in tool-level Chinese description. 16 new workflow.rs unit tests (tests::auto_sonnet_* + tests::validate_auto_sonnet_args_* + tests::caller_already_chose_sonnet_* + tests::build_auto_sonnet_block_* + tests::attach_auto_sonnet_to_payload_* + tests::shape_label_*) + 4 new plan.rs unit tests (tests::validate_distill_chain_args_*auto_sonnet* + tests::json_shape_label_*). All 7 acceptance commands green: cargo test workflow::tests (142/142, 16 new) / plan::tests (275/275, 4 new) / cargo test -p missiond-daemon (1439/1439, +19 from 1420 baseline) / cargo test -p missiond-mcp --lib (17/17) / cargo build --workspace clean / check-architecture-lisp --all-v2 (20 files) / git diff --check (no whitespace). cargo test -p missiond-core baseline 327/327 (orthogonal — not touched). Pre-commit task-scope-guard --mode staged: 3 staged files all in :write-scope. Post-commit verify-task-contract OK. Report written out-of-scope to .missiond/tasks/wave21/reports/wave21-07-sonnet-distill-chain-auto-apply-v1.report.lisp (intentionally untracked per wave21 protocol).")

  (claim
    :id wave21-08-claim-001
    :task wave21-08-machine-contract-autonomous-loop-smoke-v3
    :agent claudecode
    :seq 18
    :at "2026-04-27T01:40:00+08:00"
    :touched ["crates/missiond-daemon/src/handlers/knowledge/unified_entry.rs"
              "crates/missiond-daemon/src/handlers/knowledge/plan.rs"
              "crates/missiond-daemon/src/handlers/knowledge/workstation_dispatch.rs"
              "crates/missiond-daemon/src/handlers/knowledge/agent_execution.rs"]
    :summary "Begin wave21-08: deterministic end-to-end smoke covering wave21-03..07 machine-contract autonomous loop (machine dispatch SSOT, task-contract emit→consume→verify, LLM workstation proposals applied=false invariant, LLM auto-approve requires_human=true invariant, sonnet distill chain auto-apply gate skipped without dual opt-in, execution report verifier accepts aligned report). No LLM, no spawn, no shell, no markdown dependence. Smoke uses fixture task-contract / report / shared-memory text projected through the existing parsers (parse_task_contract / read_report_summary / read_task_contract_id) plus pure helpers (build_artifact_refs / decorate / parse_workstation_proposals / classify_proposal_safety / enforce_proposal_invariants / build_distill_chain_block) so each invariant is pinned without spinning AppState. Malformed task / report / memory data MUST surface structured failures (TASK_CONTRACT_MALFORMED / TASK_REPORT_MALFORMED / TASK_REPORT_TASK_ID_MISMATCH / TASK_REPORT_COMMIT_HASH_MISMATCH).")

  (completion
    :id wave21-08-completion-001
    :task wave21-08-machine-contract-autonomous-loop-smoke-v3
    :agent claudecode
    :seq 19
    :at "2026-04-27T02:10:00+08:00"
    :touched ["crates/missiond-daemon/src/handlers/knowledge/unified_entry.rs"
              "crates/missiond-daemon/src/handlers/knowledge/plan.rs"
              "crates/missiond-daemon/src/handlers/knowledge/workstation_dispatch.rs"
              "crates/missiond-daemon/src/handlers/knowledge/agent_execution.rs"
              ".missiond/tasks/wave21/reports/wave21-08-machine-contract-autonomous-loop-smoke-v3.report.lisp"
              ".missiond/tasks/wave21/shared-memory.lisp"]
    :summary "Committed wave21-08 (commit 8ba872365a4b, 4 files / +1201 / -0). Deterministic end-to-end smoke covering the wave21 machine-contract autonomous loop across the 4 handler files in :write-scope. 15 new tests grouped by handler: unified_entry.rs adds 3 (smoke_wave21_machine_contract_autonomous_loop_envelope_pins_every_invariant — single envelope round-trip pinning row-id pointers + machine-contract splice (task_contract_path == task_contract_source_path SSOT) + Wave 21/04 workstation_proposals applied=false / auto_spawn=false / I3-04 + Wave 21/06 LLM auto-approve decision != rejected / requires_human=true / applied=false / I1-06 + I3-06 + Wave 21/07 default-off byte-shape (no auto_sonnet block when not requested) / I1-07 + Wave 19/20 distill_chain triggered=true status=recorded chain_mode=record_only / I3-07 + v0 non-goals loud; smoke_wave21_llm_auto_approve_destructive_action_pins_requires_human — destructive action archive pins requires_human=true regardless of model output / I2-06 + destructive_check sourced from is_destructive_review_action (not model) / I5-06; smoke_wave21_machine_contract_loop_proof_markdown_is_non_load_bearing — task_brief_preview MUST stay on inner payload, NOT in artifact_refs (machine-mode brief is non-load-bearing); brief preview still surfaces ## Source contract preamble on inner payload). plan.rs adds 3 (smoke_wave21_distill_chain_block_pins_skip_status_when_trigger_short_circuits — wave21-07 I3 the gate REUSES wave-20 trigger outcomes; skip_plan_not_succeeded MUST NOT fabricate evidence_path / chain_index_in_plan / distill_result; smoke_wave21_distill_chain_block_surfaces_inner_distill_warning — recorded_with_distill_warning surfaces both inner result + warning; wave21-07 I7 additive contract; smoke_wave21_machine_dispatch_response_pins_source_path_invariant — build_workstation_dispatch_response in machine mode surfaces task_contract_source_path verbatim + brief preview carries ## Source contract preamble + wave21-04 dispatch_contract_mode marker is load-bearing). workstation_dispatch.rs adds 3 (smoke_wave21_workstation_proposals_pin_applied_false_invariant_across_safety_spectrum — fixture spans Safe (target=mission_task_delegate) + InvalidStrategy (vibes-based) + Safe objective; applied=false on every proposal + bundle auto_spawn=false; classify_proposal_safety(target, mission_unknown) MUST tag UnsupportedTarget; wave21-04 I3 + I4 + I5; smoke_wave21_workstation_proposal_unavailable_pins_no_silent_fallback — Sonnet unavailable surfaces status=llm_unavailable + reason naming `no fallback` / `claude -p` / `prompt mode`; auto_spawn=false pinned even on llm_unavailable; smoke_wave21_machine_contract_overlay_drives_brief_renderer — parse_task_contract → overlay_contract → build_task_brief_with_source: contract :goal beats stale caller objective + :scope wins + :write-scope replaces stale owned_files + :must-not-touch surfaces verbatim + :commit :policy reaches hints + :dispatch-strategy beats caller fresh-code-alignment + brief carries ## Source contract preamble + agent-team hint exactly once). agent_execution.rs adds 6 (smoke_wave21_machine_contract_autonomous_loop_verifier_accepts_aligned_triple — wave21-03 happy path: aligned (contract, report, hash) triple passes verifier + summary echoes resolved paths + :checked surfaces all 5 wave21-03 rules; smoke_wave21_malformed_report_task_id_yields_structured_failure — mismatched :task_id MUST surface TASK_REPORT_TASK_ID_MISMATCH; smoke_wave21_malformed_task_contract_yields_structured_failure — schema mismatch MUST surface TASK_CONTRACT_MALFORMED; smoke_wave21_missing_report_yields_structured_failure — missing report file MUST surface TASK_REPORT_REQUIRED; smoke_wave21_mismatched_commit_hash_yields_structured_failure — mismatched commit_hash MUST surface TASK_REPORT_COMMIT_HASH_MISMATCH; smoke_wave21_report_and_contract_projectors_extract_required_fields — read_report_summary extracts schema/task_id/commit_hash + read_task_contract_id extracts head id + cross-check anchor: contract head id == report :task_id). All 6 acceptance commands green: cargo test handlers::knowledge::unified_entry::tests (67/67, +3) / handlers::knowledge::agent_execution::tests (108/108, +6) / cargo test -p missiond-daemon (1454/1454, +15 from 1439 baseline) / cargo build --workspace clean / check-architecture-lisp --all-v2 (20 files) / git diff --check (no whitespace). Pre-commit task-scope-guard --mode staged: 4 staged files all in :write-scope. Post-commit verify-task-contract: 8ba872365a4b against contract OK (commit message + scope check write-scope-only + must-not-touch clean + acceptance presence all OK). Cross-wave invariants pinned in this smoke: I1-06 (LLM auto-approve NEVER rejected) / I2-06 (destructive_blocked) / I3-06 (applied=false + requires_human=true) / I5-06 (destructive_check sourced from is_destructive_review_action) / I3-04 (workstation proposals applied=false + bundle auto_spawn=false) / I1-07 (auto_sonnet default-off byte-shape) / I3-07 (chain reuses wave-20 trigger outcomes) / I7-07 (chain block additive) / wave-19 task-contract / wave-20 dispatch-contract / wave21-03 verifier 5-rule cross-check. Markdown is non-load-bearing — task_brief_preview NEVER projected into artifact_refs. Report written out-of-scope to .missiond/tasks/wave21/reports/wave21-08-machine-contract-autonomous-loop-smoke-v3.report.lisp (intentionally untracked per wave21 protocol).")

  (claim
    :id wave21-09-claim-001
    :task wave21-09-lisp-backfill-wave21-status
    :agent claudecode
    :seq 20
    :at "2026-04-27T02:30:00+08:00"
    :touched [".missiond/v2/intent-machine-contract.lisp"
              ".missiond/v2/intent-pillar-source-index.lisp"
              ".missiond/v2/intent-flow.lisp"
              ".missiond/v2/intent-intent-layer.lisp"
              ".missiond/v2/intent-tools.lisp"
              ".missiond/v2/intent-plan-dag.lisp"
              ".missiond/v2/intent-workstation-policy.lisp"
              ".missiond/v2/intent-execution-governance.lisp"
              ".missiond/v2/intent.lisp"]
    :summary "Begin wave21-09: backfill MissionD v2 Lisp for wave 21 task 01-08 真实 committed implementation truth. machine-contract layer 升 v0.3 → v0.4; flow / intent-layer / tools / plan-dag / workstation-policy / execution-governance / intent.lisp 各 status 句子追加 wave 21 propose+apply-gate 范式段落; intent.lisp navigation-assets +8 wave21 entries (hooks-path-installer-v1 / task-run-verifier-v1 / execution-report-verifier-integration-v1 / autonomous-workstation-llm-proposal-v0 / plan-inference-apply-gate-v1 / llm-auto-approve-proposal-v0 / sonnet-distill-chain-auto-apply-v1 / machine-contract-autonomous-loop-smoke-v3); intent-pillar-source-index.lisp 新增 wave-21-backfill v1.2 块 (区域 69-78: 8 anchor entry + 2 status-upgrade — review-automation-policy-v0 note 扩 第三 review knob + task-contract-v1 note 扩 真正 6 段闭环); deferred-coverage v1.2 paragraph + wave-21-status-summary 11 项 + wave21-task-09-non-goal 12 项 + pre-compression-checklist wave 21 task 09 +8 entry; next-step 重写 wave 21 propose+apply-gate paradigm next-推进路径. Honest accounting: hooks-installer 仍 opt-in repo-local only / workstation propose 永不 auto-spawn / plan inference apply gate persisted plan.sexp_text 永不 mutate / LLM auto-approve propose-only requires_human=true / sonnet distill chain 仍 dual opt-in required — 全部照实记录, 不假装完整 apply.")

  (completion
    :id wave21-09-completion-001
    :task wave21-09-lisp-backfill-wave21-status
    :agent claudecode
    :seq 21
    :at "2026-04-27T02:50:00+08:00"
    :touched [".missiond/v2/intent-machine-contract.lisp"
              ".missiond/v2/intent-pillar-source-index.lisp"
              ".missiond/v2/intent-flow.lisp"
              ".missiond/v2/intent-intent-layer.lisp"
              ".missiond/v2/intent-tools.lisp"
              ".missiond/v2/intent-plan-dag.lisp"
              ".missiond/v2/intent-workstation-policy.lisp"
              ".missiond/v2/intent-execution-governance.lisp"
              ".missiond/v2/intent.lisp"
              ".missiond/tasks/wave21/reports/wave21-09-lisp-backfill-wave21-status.report.lisp"
              ".missiond/tasks/wave21/shared-memory.lisp"]
    :summary "Committed wave21-09 (commit 9670e1f4627c, 9 files / +346 / -30). 9 v2 lisp files updated reflecting 8 wave-21 commits truthfully (44c74df hooks installer / 1335fa7 run verifier / 308426e execution report-verifier integration / 68b84f1 autonomous workstation LLM proposal / a18200b plan inference apply gate / e140773 LLM auto-approve proposal / 4d494db sonnet distill chain auto-apply / 8ba8723 machine-contract autonomous loop smoke v3). intent-machine-contract.lisp: layer 升 v0.3 → v0.4 + status 段加 wave 21 task 01-08 完整 narrative + current-files 加 3 wave21 additions (hooks-installer / hooks-doctor / run-verifier) + non-goals-v0 加 5 项明确 (Markdown 二度钉死 / propose-only persisted state / hooks opt-in repo-local) + next-steps 加 8 wave21 DONE 项 + 8 remaining future 项. intent-pillar-source-index.lisp: 新增 wave-21-backfill v1.2 块 (区域 69-78) — 8 anchor entry (hooks-path-installer-v1 / task-run-verifier-v1 / execution-report-verifier-integration-v1 / autonomous-workstation-llm-proposal-v0 / plan-inference-apply-gate-v1 / llm-auto-approve-proposal-v0 / sonnet-distill-chain-auto-apply-v1 / machine-contract-autonomous-loop-smoke-v3) + 2 status-upgrade (review-automation-policy-v0 note 扩 加 第三 review knob auto_approve_mode / task-contract-v1 note 扩 加 真正 6 段闭环 brief→preflight→staged guard→commit→verifier→execution complete verify); deferred-coverage v1.2 paragraph; wave-21-status-summary 11 项; wave21-task-09-non-goal 12 项; pre-compression-checklist wave 21 task 09 +8 entry; next-step 重写 wave 21 propose+apply-gate paradigm next 推进路径. flow / intent-layer / tools / plan-dag / workstation-policy / execution-governance / intent.lisp pillar status 句子各追加 wave 21 段落 (machine-contract / 6 段闭环 / propose+apply-gate / propose-only invariants 钉死). intent.lisp navigation-assets +8 wave21 cluster + worker pillar :canonical-status 加 wave 21-03/04 / tools pillar 加 wave 21-01..08 / memory pillar 加 wave 21-03 + 21-06 三 review knob; precompression-note 加 wave 21 task 09. **3 acceptance commands all green**: node scripts/check-architecture-lisp.mjs --all-v2 → 20 files OK; node scripts/check-task-contract.mjs --all → 37 tasks OK; git diff --check (9 files) → no whitespace errors. Pre-commit task-scope-guard --mode staged: 9 staged files all in :write-scope. Post-commit verify-task-contract: 9670e1f4627c against contract OK. Anchors updated: machine-contract-layer (v0.4) + machine-contract.task-contract-v1 (note 扩) + unified-entry-pipeline.review-automation-policy-v0 (note 扩) + plan-dag-shard / workstation-policy-shard / execution-governance-shard / intent-flow / intent-intent-layer / intent-tools status 句子. Proposal-only vs applied status distinctions reflected as code-aligned-partial: hooks-path-installer-v1 (opt-in repo-local only — 不擅自 default-on); autonomous-workstation-llm-proposal-v0 (propose only / applied=false / auto_spawn=false 永钉死); plan-inference-apply-gate-v1 (apply gate 已落但 persisted plan.sexp_text 永不 mutate); llm-auto-approve-proposal-v0 (propose only / applied=false + requires_human=true 永钉死); sonnet-distill-chain-auto-apply-v1 (auto-apply 已落但仍 dual opt-in required). Pending 完整 apply: workstation true spawn / persisted plan inference apply / LLM auto-approve apply / hooks default-on / sonnet 完全自动接 chain / report-contract checker auto-invoke from daemon / frontend Lisp. Report written out-of-scope to .missiond/tasks/wave21/reports/wave21-09-lisp-backfill-wave21-status.report.lisp (intentionally untracked per wave21 protocol — :write-scope is v2 lisp only)."))
