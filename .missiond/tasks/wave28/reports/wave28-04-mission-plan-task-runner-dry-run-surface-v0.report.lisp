;; Wave 28 / Task 04 — mission_plan task-runner dry-run surface v0.
;; Schema: missiond.report-contract.v1
;; Source: .missiond/tasks/wave28/wave28-04-mission-plan-task-runner-dry-run-surface-v0.lisp

(report wave28-04-mission-plan-task-runner-dry-run-surface-v0
  :schema "missiond.report-contract.v1"
  :task_id "wave28-04-mission-plan-task-runner-dry-run-surface-v0"
  :status done
  :commit_hash "dde6f61df560"
  :files_changed
    ["crates/missiond-daemon/src/handlers/knowledge/plan.rs"
     "crates/missiond-mcp/src/tools/knowledge/plan.rs"]

  :acceptance_results
    [(:command "cargo test -p missiond-daemon --bin missiond task_runner"
      :exit_code 0
      :ok true
      :notes "13 new task_runner tests PASS (1667 baseline filtered out): task_runner_off_default_does_no_file_io / task_runner_dry_run_with_seed_manifest_emits_block / task_runner_dry_run_topological_batches_correct / task_runner_dry_run_overlap_default_reject / task_runner_dry_run_overlap_warn_policy / task_runner_dry_run_missing_manifest_emits_status_missing / task_runner_dry_run_malformed_manifest_emits_status_malformed / task_runner_apply_returns_invalid_param / task_runner_auto_returns_invalid_param / task_runner_unknown_returns_invalid_param / task_runner_dry_run_does_not_change_dispatch / applied_remains_false_with_task_runner / task_runner_off_with_runner_args_and_router_args_does_no_file_io.")
     (:command "cargo test -p missiond-daemon --bin missiond"
      :exit_code 0
      :ok true
      :notes "Full daemon test suite: 1680 passed; 0 failed. Equals wave27-06 baseline 1667 + 13 new wave28-04 tests. No regressions in any pre-existing surface (router_recommendation / router_dispatch_descriptor / inference / apply-gate / DAG / sysinfra / store / workers).")
     (:command "cargo test -p missiond-mcp --lib"
      :exit_code 0
      :ok true
      :notes "MCP lib tests: 17 passed (matches wave-15..27 baseline). The new task_runner_mode + task_runner_manifest_path properties land inside the existing mission_plan tool definition without changing the registered tool count or any pre-existing schema.")
     (:command "cargo build --workspace"
      :exit_code 0
      :ok true
      :notes "Workspace build clean. 86 pre-existing warnings (dead-code, sqlx-postgres future-incompat) — all from prior code. No new warnings introduced by wave28-04.")
     (:command "node scripts/check-task-contract.mjs --all"
      :exit_code 0
      :ok true
      :notes "task-contract check OK (98 tasks).")
     (:command "git diff --check -- crates/missiond-daemon/src/handlers/knowledge/plan.rs crates/missiond-mcp/src/tools/knowledge/plan.rs"
      :exit_code 0
      :ok true
      :notes "No whitespace errors on either edited file.")
     (:command "node scripts/check-missiond-hooks.mjs --json"
      :exit_code 0
      :ok true
      :notes "preflight OK; core.hooksPath aligned to .githooks; pre-commit hook present and executable.")
     (:command "node scripts/task-scope-guard.mjs --task .missiond/tasks/wave28/wave28-04-mission-plan-task-runner-dry-run-surface-v0.lisp --mode staged"
      :exit_code 0
      :ok true
      :notes "task-scope-guard staged OK (2 staged file(s)) — both inside :write-scope; zero matches against :must-not-touch (plan_dag.rs / workstation_dispatch.rs / agent_execution.rs / workflow.rs / directive.rs / .missiond/v2/** / .missiond/tasks/** / .missiond/claudecode/** / scripts/**).")
     (:command "node scripts/verify-task-contract.mjs .missiond/tasks/wave28/wave28-04-mission-plan-task-runner-dry-run-surface-v0.lisp"
      :exit_code 0
      :ok true
      :notes "task-contract verify OK against dde6f61df560 — commit hash exists; commit message matches `feat(plan): surface task runner manifest dry-run` per contract; changed_files (2: crates/missiond-daemon/src/handlers/knowledge/plan.rs, crates/missiond-mcp/src/tools/knowledge/plan.rs) ⊆ write-scope (write-scope-only); changed_files ∩ must-not-touch = ∅; acceptance commands present in contract.")]

  :scope_deviations []

  :trace_refs [wave28-04-trace-start-001 wave28-04-trace-commit-001 wave28-04-trace-complete-001]

  :major_decisions
    [(:decision "Mirror the wave27-03 split: separate `compute_runner_block` (pure projection from manifest) from `attach_task_runner_block` (response-block splice + Off-path early-return)."
      :rationale "wave27-03 already proved this layering keeps response-shape decisions decoupled from parsing. Splitting makes the new code testable in isolation (the 13 tests all hit attach_task_runner_block + parse_task_runner_mode without needing to drive the live execute handler), and matches the file's existing house style. The off-mode early-return MUST live inside attach (not inside compute) so a typo in the manifest path can never trigger a stat() call when the operator opted out.")
     (:decision "Re-implement the S-expression tokeniser inside the new task_runner_dry_run module instead of factoring out a shared one."
      :rationale "The wave24-04 router-policy tokeniser is local to its module by design (handlers/knowledge/plan.rs is a 22k-line file; cross-module surface is minimised). Extracting a shared tokeniser would touch lib-public surface and risk dragging the wave26-03 backend-registry parser, the wave27-03 descriptor projector, and the wave28-04 manifest parser into one shared codebase. Future consolidation can happen if a third consumer appears; for v0 the duplication is local and explicit, which matches the wave24-04 + wave26-03 lesson that small self-contained tokenisers are easier to audit than shared schemas.")
     (:decision "Pseudo-Lisp parser tolerates BOTH `(node :task_id <id> ...)` AND `(node <id> :task_id <id> ...)` shapes."
      :rationale "Wave28-01's manifest schema canonicalises :task_id as a keyword field, but several wave28-02 fixtures emit the id as a positional atom right after `node`. A v0 parser that rejected the positional shape would force every manifest author to choose one canonical form before the wave28-06 loop can land. Tolerating both keeps the read surface compatible with any wave28 fixture or hand-authored manifest. The wave28-01 Node checker still validates strictly — the daemon's reader is intentionally LENIENT for the read path so dry-run telemetry never hides a malformed manifest behind a parser disagreement.")
     (:decision "Manifest read failures (missing / unreadable / malformed) emit a deterministic empty projection alongside the manifest_status enum, NOT an empty block."
      :rationale "Brief mandates non-fatal failure handling and continued dispatch. Returning a full block with empty arrays + zero counters lets downstream consumers rely on the field-set unconditionally; they never have to write `if (block.batches) ...`. Combined with the mandatory `task_runner_warning` field on every degraded variant, the operator can tell the manifest is degraded WITHOUT having to inspect for absent fields. This matches the wave26-03 BackendRegistryInfo pattern (Missing / Unreadable / Malformed all surface a structured status + warning).")
     (:decision "applied=false is hard-coded as `Value::Bool(false)` inside `build_runner_response_block` and asserted as a literal-bool invariant in 3 tests (applied_remains_false_with_task_runner, task_runner_dry_run_with_seed_manifest_emits_block, task_runner_dry_run_missing_manifest_emits_status_missing)."
      :rationale "Cross-wave invariant pinned since wave24-04. The task contract explicitly demands `applied=false` as a literal — never computed, never derived. Multiple test asserts (assert_eq!(block[applied], Value::Bool(false)) AND assert!(block[applied].is_boolean())) catch BOTH a future change that flips the value AND a future change that turns it into a string. Mirrors the wave27-06 pattern for the descriptor's three locked literal-bool invariants.")
     (:decision "The combined Off-mode invariant test (`task_runner_off_with_runner_args_and_router_args_does_no_file_io`) supplies BOTH wave-28 task_runner args AND ALL FOUR wave-24..27 router args (router_policy_path / router_policy_trace_index_path / router_backend_registry_path / router_dispatch_descriptor) with bogus paths, then asserts NONE of the bogus paths exist after the call AND the response is byte-identical to the pristine baseline."
      :rationale "Brief mandates this combined invariant: proves the Off-path early-return precedes BOTH the new wave28-04 manifest read AND the prior wave-24..27 reads. Without this test, a future regression that moves the manifest read to a pre-attach early-eval site (or moves the trace-index read out of compute_recommendation) would not be caught by the simpler per-read tests. The bogus-paths-must-not-exist assertion is the strongest available proof short of mounting tmpfs (which is impractical in unit tests).")]

  :time_sinks
    [(:label "Reading wave24-04 + wave25-03 + wave26-03 + wave27-03 router-policy implementation to mirror the split"
      :notes "router_policy_dry_run is now a 1.8k-line module covering 4 layers (parse mode → compute_recommendation → compute_recommendation_block → attach_router_recommendation_block). Walking it end-to-end was needed to identify the correct insertion points in handle_execute (line 6229 for parse, line 6751 for attach) AND to reuse the compute_X / attach_X naming + the same is_error guard / first()-content destructure / serde_json::from_str fall-through pattern. The wave27-06 self-audit grep test was a useful reference for what NOT to write into the new code (no shell, no spawn, no git, no LLM client).")
     (:label "Designing the in-Rust manifest projector to mirror the wave28-02 Node CLI's planFromManifestObject"
      :notes "Topological batching with dispatch_group boundaries is the load-bearing piece (wave28-02 uses a Map-of-groups inside the ready loop). Translated to BTreeMap<String, Vec<String>> for stable iteration order. Critical-path is a memoised DFS over the dependents map (matching wave28-02). Overlap diagnostics use a (group, taskA, taskB) key with set-of-paths value, then sorted Vec<OverlapDiagnostic>. Verification-tier counts pre-seeded so a tier with zero nodes still appears (deterministic shape, matches wave28-02).")
     (:label "Writing 13 tests that exercise both the parse + attach surfaces AND the projector"
      :notes "task_runner_off_default_does_no_file_io covers the byte-identical invariant + the `must-not-create-bogus-path` assertion. task_runner_dry_run_with_seed_manifest_emits_block / task_runner_dry_run_topological_batches_correct exercise the projector directly. task_runner_dry_run_overlap_default_reject + task_runner_dry_run_overlap_warn_policy pin severity transitions. task_runner_dry_run_missing_manifest_emits_status_missing + task_runner_dry_run_malformed_manifest_emits_status_malformed pin the non-fatal path AND assert dispatch fields are unchanged. task_runner_apply / auto / unknown_returns_invalid_param pin the INVALID_PARAM gate before plan lookup. task_runner_dry_run_does_not_change_dispatch + applied_remains_false_with_task_runner re-pin the dispatch byte-identical + applied=false invariants. task_runner_off_with_runner_args_and_router_args_does_no_file_io is the strongest combined invariant.")]

  :unexpected_work
    [(:summary "Tolerated optional-positional task-id node shape in the manifest parser. Several Lisp manifests in flight write `(node :task_id node-a ...)` while others write `(node node-a :task_id node-a ...)`. The parser handles both shapes by checking whether the first item after `node` is a Keyword (canonical form) or an Atom/Str (positional id). This was not in the brief but is necessary to keep the daemon's read surface compatible with any wave28 fixture or hand-authored manifest. The wave28-01 Node checker still enforces the canonical form.")
     (:summary "Added the `manifest_path` field to the response block when the path was supplied (echo of the input). Brief lists the required fields but echoing the input path is a wave-25 / wave-26 convention worth preserving so downstream telemetry can correlate the read with its source. Field is omitted when no path was supplied (dry_run + no manifest_path = degraded with a `task_runner_warning` saying the path is required).")
     (:summary "Added `task_runner_warning` field on EVERY degraded variant (missing / unreadable / malformed / no-path-supplied), not just on the failure modes the brief explicitly named. This matches the wave25-03 / wave26-03 trace_index_warning + backend_warning convention so the operator never has to inspect for an absent field to detect degradation.")]

  :recommended_backend "claudecode"
  :router_confidence "high"
  :router_policy_path ".missiond/router/router-policy-v1.lisp"
  :router_dry_run_only true
  :router_applied false
  :router_reasons
    ["Dispatch strategy fresh-code-alignment + owner claudecode → matches r-fresh-code-alignment-to-claudecode in router-policy-v1 (priority 100, single matched rule)."
     "Code-alignment surface (Rust-only edits inside crates/missiond-{daemon,mcp}/.../plan.rs) is the canonical claudecode beat — mirrors the wave24-04 / wave25-03 / wave26-03 / wave27-03 pattern."
     "Router output is recorded for telemetry only; runtime dispatch unchanged (claudecode is the live default and remained the live default for this task)."]
  :router_trace_index_path ".missiond/router/trace-index-v1.lisp"

  :router_backend_readiness_status "current-default"
  :router_backend_runtime_allowed true
  :router_apply_eligible false
  :router_apply_blockers
    ["current-default is the live runtime today but explicit runtime-ready opt-in is required upstream before this task's dispatch handoff would mark a descriptor as apply-eligible (wave27-01 eligibility-gate intentionally REJECTS current-default → eligible)."]
  :router_backend_registry_path ".missiond/router/router-backend-registry-v1.lisp"

  :notes
    "wave28-04 ships the dry-run-only mission_plan task-runner manifest surface. Two new optional MCP args (task_runner_mode ∈ {off, dry_run}; task_runner_manifest_path string), validated BEFORE plan lookup so apply / auto / unknown / non-string values reject with structured INVALID_PARAM. The handler attaches a new `task_runner` block on the response when mode=dry_run AND a manifest path is supplied; off/default mode is byte-identical to the wave-15..27 baseline AND does ZERO file I/O even when a manifest path is supplied (the Off-path early-return in attach_task_runner_block precedes any file read).

     Response block top-level fields: schema (literal `missiond.task-runner-plan.v0`), manifest_status ∈ {used, missing, unreadable, malformed}, applied (LOCKED literal `Value::Bool(false)`, never computed), manifest_path (echo when supplied), task_runner_warning (only on degraded variants), wave (manifest header echo), productive_only (manifest header echo as bool), batches (Vec<Vec<String>> topological grouping by dependency depth + dispatch_group boundary), critical_path_minutes (longest dependency chain by estimated_minutes), total_estimated_minutes (sum of all node estimates), verification_tier_counts ({local, smoke, full} with all keys pre-seeded so a tier with zero nodes still appears), overlap_diagnostics (Vec<{pair, paths, severity, group}> for same-dispatch_group write-scope overlaps; severity follows manifest :overlap_policy — reject → error, warn → warning).

     Mirror of wave27-03's split: compute_runner_block (pure projection of RunnerInputs) + attach_task_runner_block (response-block splice with Off-path early-return) + load_runner_inputs (file I/O isolation). The projector internally calls project_manifest which mirrors wave28-02's planFromManifestObject (topological batching, critical-path memoised DFS, overlap diagnostics with sorted (group, taskA, taskB) key). Manifest parser is a self-contained S-expression tokeniser (mirrors the wave24-04 in-module pattern; minimal subset extracting wave / productive_only / overlap_policy / per-node task_id / depends_on / dispatch_group / verification_tier / estimated_minutes / write_scope).

     Hard guarantees pinned by tests:
       1. parse_task_runner_mode: apply / auto / unknown / non-string all return ToolResult INVALID_PARAM error (3 dedicated tests).
       2. mode=off OR mode-absent with bogus manifest_path: response byte-identical to baseline AND the bogus path file does NOT exist after the call (task_runner_off_default_does_no_file_io).
       3. mode=off + ALL of {task_runner_manifest_path, router_policy_path, router_policy_trace_index_path, router_backend_registry_path, router_dispatch_descriptor=true} supplied with bogus paths: response byte-identical to pristine baseline AND none of the 4 bogus paths exist (task_runner_off_with_runner_args_and_router_args_does_no_file_io).
       4. mode=dry_run + valid manifest: response carries task_runner.applied = literal Value::Bool(false); is_boolean() asserted alongside == false (catches a future regression that turns it into a string) (applied_remains_false_with_task_runner + task_runner_dry_run_with_seed_manifest_emits_block).
       5. Topological batches respect BOTH dependency depth AND dispatch_group boundary (task_runner_dry_run_topological_batches_correct: 3 batches for the 3-node 2-group fixture — group A first by lex order, then group B at depth 0, then group A at depth 1).
       6. Overlap-policy default reject → severity=error; explicit warn → severity=warning (task_runner_dry_run_overlap_default_reject + task_runner_dry_run_overlap_warn_policy).
       7. Manifest missing / malformed → manifest_status enum value + task_runner_warning + dispatch unchanged + applied=false literal (task_runner_dry_run_missing_manifest_emits_status_missing + task_runner_dry_run_malformed_manifest_emits_status_malformed).
       8. mode=dry_run NEVER changes dispatch fields (task_runner_dry_run_does_not_change_dispatch).

     Cross-wave invariants honored:
       * NO worker spawn (no Tokio task / std thread / process spawn).
       * NO Node child_process / shell-out (NO scripts/plan-task-runner.mjs invocation; pure-Rust mirror).
       * NO git mutation (pure file read).
       * NO mission_task_delegate invocation.
       * NO LLM call (pure parser + projector).
       * applied=false is hard-coded `Value::Bool(false)` in build_runner_response_block — never computed.
       * Off/default mode is byte-identical with NO file I/O even when ALL prior wave-24..27 router args AND the new wave-28 task_runner args are supplied with bogus paths (combined invariant test proves the Off-path early-return precedes ALL reads).
       * Manifest parser tolerated keys: keys outside the wave28-04 projection target (e.g. :brief_mode, :shared_preamble_path, :description, :generated_at, :generator, :heartbeat_minutes, :notes, :owner, :kind) are silently ignored so future schema growth does not break the daemon.

     Edited files (within :write-scope):
       * crates/missiond-daemon/src/handlers/knowledge/plan.rs — adds parse_task_runner_mode pre-flight (line 6242 area), attach_task_runner_block post-pass after attach_router_recommendation_block (line 6770 area), the new task_runner_dry_run module (~640 LOC inserted between the existing router_policy_dry_run module and the tests module), and 13 new tests inside handlers::knowledge::plan::tests.
       * crates/missiond-mcp/src/tools/knowledge/plan.rs — adds 2 properties (task_runner_mode enum + task_runner_manifest_path string) to build_properties() with descriptions explicitly stating `dry-run only` and `no execution` per contract requirement #6.

     Pre-commit pipeline: cargo test -p missiond-daemon --bin missiond task_runner (exit=0, 13/13 PASS) → cargo test -p missiond-daemon --bin missiond (exit=0, 1680/1680 PASS, 1667 baseline + 13 new) → cargo test -p missiond-mcp --lib (exit=0, 17/17 PASS, baseline preserved) → cargo build --workspace (exit=0) → check-task-contract.mjs --all (exit=0, 98 tasks) → git diff --check (exit=0) → check-missiond-hooks.mjs --json (preflight aligned) → git add (2 paths) → task-scope-guard.mjs --mode staged (OK, 2 staged) → MISSIOND_TASK_CONTRACT=... git commit -m \"feat(plan): surface task runner manifest dry-run\" (commit dde6f61df560) → verify-task-contract.mjs (OK against dde6f61df560).

     Append-only ledger updates: shared-memory wave28-04-claim-001 (seq 9) before staging + wave28-04-completion-001 (seq 10) after verify; session-trace wave28-04-trace-start-001 (seq 12) before reading background + wave28-04-trace-commit-001 (seq 15, with commit hash) + wave28-04-trace-complete-001 (seq 16). Both ledgers re-validated after each append.

     Constraints honored: NO edits to plan_dag.rs / workstation_dispatch.rs / agent_execution.rs / workflow.rs / directive.rs (must-not-touch list). NO edits to .missiond/v2/** / .missiond/tasks/** / .missiond/claudecode/** / scripts/** beyond the append-only shared-memory + session-trace ledgers (both writable per :session-trace-writable true and shared-memory append-policy). Did not git add . / git push / --no-verify / --amend / --force.

     Parallel coordination: wave28-05 (task-runner batch verifier) ran concurrently in dispatch_group C against scripts/verify-task-runner-batch.mjs + scripts/verify-task-run.mjs — zero file overlap with wave28-04's crates/missiond-{daemon,mcp}/.../plan.rs. Both agents appended to the same shared-memory + session-trace ledgers (read fresh, append-only).")
