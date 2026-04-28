;; Wave 28 / Task 05 — Task runner batch verifier v0.
;; Schema: missiond.report-contract.v1
;; Source: .missiond/tasks/wave28/wave28-05-task-runner-batch-verifier-v0.lisp

(report wave28-05-task-runner-batch-verifier-v0
  :schema "missiond.report-contract.v1"
  :task_id "wave28-05-task-runner-batch-verifier-v0"
  :status done
  :commit_hash "fe70a5fcad04"
  :files_changed
    ["scripts/verify-task-runner-batch.mjs"]

  :acceptance_results
    [(:command "node scripts/verify-task-runner-batch.mjs --dry-fixture"
      :exit_code 0
      :ok true
      :notes "task-runner-batch verify fixtures OK (8 cases): pass-all-green-2-node, fail-missing-report, fail-missing-memory-completion, fail-commit-hash-mismatch, pass-pseudo-node-skipped (archive id + backfill kind), pass-productive-only-false-skips-all, pass-deterministic-byte-identical-json (re-run yields byte-identical JSON), fail-contract-message-mismatch (delegated to wave23-03 verifyRun). All assertions hit aggregate_status / verified_nodes / total_nodes / missing_reports / missing_memory_completions / failed_contract_verifications / skipped_nodes; the deterministic case re-runs verifyManifest twice and compares stableStringify bytes.")
     (:command "node scripts/verify-task-run.mjs --dry-fixture"
      :exit_code 0
      :ok true
      :notes "wave23-03 verifier baseline preserved byte-identically: 19 fixtures + 15 helper cases (7 commitHashMatches + 8 commitHashesAgree). NO edits to scripts/verify-task-run.mjs were required — all helpers needed by the batch verifier (loadReport, loadReportFromSource, loadLedger, loadLedgerFromSource, verifyRun, commitHashMatches, commitHashesAgree) were already named-exported by wave23-03.")
     (:command "node scripts/check-task-contract.mjs --all"
      :exit_code 0
      :ok true
      :notes "task-contract check OK (98 tasks) — wave22..wave28 task contracts including wave28-05 parse and pass shape / scope / must-not-touch / acceptance / commit-policy validation. No regressions vs the wave28-03 baseline.")
     (:command "git diff --check -- scripts/verify-task-runner-batch.mjs scripts/verify-task-run.mjs"
      :exit_code 0
      :ok true
      :notes "no whitespace errors on either path; trailing-newline / tab-stop hygiene clean. scripts/verify-task-run.mjs was NOT touched (zero diff). Only scripts/verify-task-runner-batch.mjs was added.")
     (:command "node scripts/check-missiond-hooks.mjs --json"
      :exit_code 0
      :ok true
      :notes "preflight OK; core.hooksPath aligned to .githooks; .githooks/pre-commit exists and is executable; no install required.")
     (:command "node scripts/task-scope-guard.mjs --task .missiond/tasks/wave28/wave28-05-task-runner-batch-verifier-v0.lisp --mode staged"
      :exit_code 0
      :ok true
      :notes "task-scope-guard staged OK: wave28-05-task-runner-batch-verifier-v0 (1 staged file) — staged path inside :write-scope; zero matches against :must-not-touch (crates/** .missiond/v2/** .missiond/router/** .missiond/tasks/wave27/** .missiond/tasks/wave28/wave28-*.lisp .missiond/tasks/wave28/dispatch-plan.lisp .missiond/claudecode/** scripts/{check-task-runner-manifest,plan-task-runner,render-wave-briefs,render-claudecode-task}.mjs).")
     (:command "node scripts/verify-task-contract.mjs .missiond/tasks/wave28/wave28-05-task-runner-batch-verifier-v0.lisp"
      :exit_code 0
      :ok true
      :notes "task-contract verify OK: wave28-05-task-runner-batch-verifier-v0 against fe70a5fcad04 — commit hash exists; commit message matches `feat(tasks): verify task runner batches` per contract; changed_files (1: scripts/verify-task-runner-batch.mjs) ⊆ write-scope (write-scope-only); changed_files ∩ must-not-touch = ∅; acceptance commands present in contract.")]

  :scope_deviations []

  :trace_refs [wave28-05-trace-start-001 wave28-05-trace-commit-001 wave28-05-trace-complete-001]

  :major_decisions
    [(:decision "scripts/verify-task-run.mjs NOT edited — every helper the batch verifier needs (loadContract, loadReport, loadLedger, loadLedgerFromSource, loadReportFromSource, verifyRun, readCommit, commitHashMatches, commitHashesAgree) was already a named export from wave23-03 / wave23-06."
      :rationale "Brief allows surgical Edit on verify-task-run.mjs only to add named exports if needed. Inspection (grep '^export') confirmed all required symbols already exported. Skipping the edit means wave23-03's 19 fixture + 15 helper baseline is mathematically byte-identical (zero diff). Avoids the risk of introducing a stale comment or import that could perturb the wave25-05 / wave26-06 / wave27-06 regression chains.")
     (:decision "Defence-in-depth skip predicate inspects BOTH :kind ∈ FORBIDDEN_PRODUCTIVE_KINDS AND :task_id substring match against ['-archive-', '-backfill-', '-index', 'lisp-backfill']."
      :rationale "Wave28-01 manifest checker rejects these as productive nodes when :productive_only is true, so in normal operation the batch verifier never sees them. But the batch verifier is also called against future-shape manifests (e.g. retry / repair manifests with :productive_only false, or manifests authored before the wave28-01 checker tightened) — silent skip is the safe default. Counted in :skipped_nodes for telemetry so an operator can confirm the skip happened on purpose.")
     (:decision "Output schema's top-level keys are emitted in alphabetical order, and array fields (missing_reports, missing_memory_completions, failed_contract_verifications, skipped_nodes) are sorted by task_id."
      :rationale "Brief mandates 'same inputs ⇒ byte-identical JSON'. JS object iteration order is insertion order, so two equivalent inputs that build the keys in different orders would otherwise differ. aggregateResults builds the object in alphabetical key order; stableStringify recurses through nested objects sorting keys; arraysEqual + sort make every list deterministic. Re-verified end-to-end with the dedicated determinism fixture (verifyManifest run twice on the same loaders, stableStringify compared character-by-character).")
     (:decision "Per-node verification is delegated to wave23-03 verifyRun rather than re-implementing contract / report / memory cross-checks."
      :rationale "verifyRun is the single source of truth for what a 'verified single task run' means (contract message + write-scope + must-not-touch + report task_id + report commit_hash + memory completion match). Re-implementing those rules in the batch verifier would create a second source of truth subject to drift. The batch verifier is now strictly a fan-out + aggregator — its only original logic is the manifest walk, the pseudo-node skip, the commit-hash-vs-memory cross-check, and the result aggregation.")
     (:decision "All filesystem and git access is funnelled through a `loaders` object so the dry-fixture path can swap in synthetic loaders (no disk, no git)."
      :rationale "Mirrors the wave23-03 fixture pattern: fixtures exercise the same pure verifyNode + aggregateResults code paths that the CLI runs, but with in-memory contract / report / ledger objects + synthetic commit info. This is the only way to assert deterministic behaviour without depending on a specific git history. realLoaders(cwd) is the single CLI binding to fs + readCommit; everything downstream is pure.")
     (:decision "Commit-hash cross-check between report :commit_hash and shared-memory completion :summary uses a regex hex-extractor (extractCommitHashFromText) instead of requiring the memory entry to declare a structured :commit_hash field."
      :rationale "Wave28 convention surfaces the commit hash inside the completion :summary text (e.g. 'Wave28-01 done at commit 145645c.') rather than as a structured field. A structured field would be cleaner but would require a schema migration to .missiond/tasks/schema/shared-memory-v1.lisp — out of scope for wave28-05. The regex extracts the first hex run of length 7..40 and compares with commitHashesAgreeLocal (mirrors wave23-03's symmetric prefix match). Mismatch → failed_contract_verifications row; only-one-side → soft pass (report side is authoritative).")]

  :time_sinks
    [(:label "Reading wave23-03 verify-task-run.mjs end-to-end to confirm reusable named exports"
      :notes "Largest sink — verify-task-run.mjs is 1342 lines and the named exports were spread across the file. Confirmed loadReport / loadLedger / verifyRun / commitHashMatches all exported and that none of the in-process verifier helpers depended on CLI-specific state. Saved an Edit pass on verify-task-run.mjs (zero diff = automatic baseline preservation).")
     (:label "Designing the synthetic loaders + fixture commit factory"
      :notes "Fixtures need to exercise verifyRun without any git access, but verifyRun expects a commitInfo with hash + message + files. syntheticCommit + syntheticLoaders together produce a verifyRun input that satisfies the wave23-03 contract verifier. The contract-message-mismatch fixture leverages this by passing a wrong-subject commit to surface verifyRun's delegated check inside the batch flow.")
     (:label "Determinism plumbing"
      :notes "Top-level alphabetical key order is naturally produced by aggregateResults; stableStringify (recursive sortKeys) guarantees nested-object keys also sort. Added a dedicated determinism fixture that re-runs verifyManifest with the same loaders and compares stableStringify bytes — direct mechanical proof that the JSON output is reproducible.")]

  :unexpected_work
    [(:summary "Added :skipped_nodes as an output field beyond the brief's required keys. The brief mandates schema / manifest_path / wave / total_nodes / verified_nodes / missing_reports / missing_memory_completions / failed_contract_verifications / aggregate_status, but a verifier that silently skips pseudo-nodes without telemetry would be opaque. :skipped_nodes lets an operator confirm the skip happened on purpose without parsing prose. Sorted, deterministic.")
     (:summary "Added a status='partial' aggregate state for the productive_only=false case. all_green requires verified_nodes==total_nodes AND total_nodes>0; failed requires at least one missing/failed entry; partial is everything else (including the zero-productive-nodes manifest). Brief allows {all_green | partial | failed} in the schema; partial is the right verdict when the manifest is orchestrator-owned and contains no productive workers.")
     (:summary "End-to-end smoke against the real wave28-01 task / report / shared-memory / commit (145645c) confirmed the verifier returns aggregate_status=all_green, verified_nodes=1, total_nodes=1, with the actual git commit available. The batch verifier is therefore live-ready for wave28-06 (loop smoke) to consume.")]

  :recommended_backend "claudecode"
  :router_confidence "high"
  :router_policy_path ".missiond/router/router-policy-v1.lisp"
  :router_dry_run_only true
  :router_applied false
  :router_reasons
    ["Dispatch strategy fresh-code-alignment + owner claudecode → matches r-fresh-code-alignment-to-claudecode in router-policy-v1 (priority 100, single matched rule)."
     "Workstation surface (NEW Node.js script + zero Rust / SQL / cargo) is the canonical claudecode beat — verifyRun delegation keeps the contract boundary clean and the batch verifier itself never touches LLM / network."
     "Router output is recorded for telemetry only; runtime dispatch unchanged (claudecode is the live default and remained the live default for this task)."]
  :router_trace_index_path ".missiond/router/trace-index-v1.lisp"

  :router_backend_readiness_status "current-default"
  :router_backend_runtime_allowed true
  :router_apply_eligible false
  :router_apply_blockers
    ["current-default is the live runtime today but explicit runtime-ready opt-in is required upstream before this task's dispatch handoff would mark a descriptor as apply-eligible (wave27-01 eligibility-gate intentionally REJECTS current-default → eligible)."]
  :router_backend_registry_path ".missiond/router/router-backend-registry-v1.lisp"

  :notes
    "wave28-05 ships scripts/verify-task-runner-batch.mjs (NEW, 1161 lines) — a read-only batch verifier that consumes a wave28-01 task-runner manifest and joins each productive node against its task contract + report + shared-memory completion + git commit. Reuses wave28-01 readManifestFile + validateManifestObject + FORBIDDEN_PRODUCTIVE_KINDS. Reuses wave23-03 verifyRun + loadContract + loadReport + loadLedger + readCommit + loadContractFromSource + loadLedgerFromSource + loadReportFromSource. scripts/verify-task-run.mjs was NOT touched (zero diff) — every helper needed was already a named export from wave23-03 / wave23-06.

     CLI: node scripts/verify-task-runner-batch.mjs --manifest <manifest.lisp> [--json] [--dry-fixture].

     Output schema (id missiond.task-runner-batch-verification.v0):
       schema                            -- pinned literal
       manifest_path                     -- as supplied (relative when given relative)
       wave                              -- from manifest header
       total_nodes                       -- count of productive nodes (skipped excluded)
       verified_nodes                    -- count of nodes that passed verifyRun
       missing_reports                   -- sorted task_id array
       missing_memory_completions        -- sorted task_id array
       failed_contract_verifications     -- sorted [{task_id, reason}, ...]
       skipped_nodes                     -- sorted task_id array (pseudo-nodes)
       aggregate_status                  -- all_green | partial | failed

     Aggregate-status decision matrix:
       all_green  ⇔ verified_nodes == total_nodes AND total_nodes > 0
                    AND missing_reports == [] AND missing_memory_completions == []
                    AND failed_contract_verifications == []
       partial    ⇔ no failures AND (verified_nodes < total_nodes OR total_nodes == 0)
       failed     ⇔ at least one missing report / missing memory completion / failed contract verification

     Pseudo-node skip (defence in depth — wave28-01 normally rejects these at manifest-validation time):
       1. node :kind ∈ {archive, backfill, index, lisp-backfill}     ⇒ skip
       2. node :task_id contains '-archive-' / '-backfill-' / '-index' / 'lisp-backfill'  ⇒ skip
       3. manifest :productive_only false                            ⇒ skip ALL nodes

     Commit-hash cross-check: extracts a hex sha (>=7 chars) from the matching shared-memory completion :summary text and compares against the report :commit_hash via commitHashesAgreeLocal (symmetric prefix match). Mismatch ⇒ failed_contract_verifications row. Only-one-side has a hash ⇒ soft pass (report side is authoritative; memory hash is advisory).

     Per-node flow:
       1. contract present at .missiond/tasks/<wave>/<task-id>.lisp           — else failed_contract_verifications
       2. report present at .missiond/tasks/<wave>/reports/<task-id>.report.lisp — else missing_reports
       3. shared-memory ledger present at .missiond/tasks/<wave>/shared-memory.lisp — else missing_memory_completions
       4. ledger contains (completion :task <task-id> ...)                       — else missing_memory_completions
       5. report :commit_hash agrees with hex sha extracted from completion :summary (when both present) — else failed_contract_verifications
       6. wave23-03 verifyRun(contract, report, ledger, commitInfo) returns ok — else failed_contract_verifications

     Determinism: top-level keys are emitted in alphabetical order; array fields are sorted by task_id; nested objects are sorted via stableStringify (recursive sortKeys). Direct proof in the dry-fixture suite (re-runs verifyManifest with the same loaders and compares stableStringify bytes character-by-character).

     Read-only invariant: NO mutating git verbs. The only git surface is readCommit (imported from verify-task-contract.mjs via verify-task-run.mjs), which uses git rev-parse | git log | git show. Synthetic loaders in --dry-fixture bypass git entirely.

     Real-world smoke: ran against a synthetic /tmp manifest naming wave28-01-task-runner-manifest-schema-v0 as its only productive node — verifier returned aggregate_status=all_green, verified_nodes=1/1 against the actual on-disk wave28-01 contract + report + shared-memory completion + git commit 145645c2fd5a.

     Dry-fixture totals: 8 cases — pass-all-green-2-node (1), fail-missing-report (1), fail-missing-memory-completion (1), fail-commit-hash-mismatch (1), pass-pseudo-node-skipped (1: archive id + backfill kind), pass-productive-only-false-skips-all (1), pass-deterministic-byte-identical-json (1), fail-contract-message-mismatch (1: delegated to wave23-03 verifyRun).

     Pre-commit pipeline: verify-task-runner-batch.mjs --dry-fixture (exit=0, 8 cases) → verify-task-run.mjs --dry-fixture (exit=0, 19+15 baseline byte-identical) → check-task-contract.mjs --all (exit=0, 98 tasks) → git diff --check (exit=0) → check-missiond-hooks.mjs --json (preflight aligned) → git add (1 path) → task-scope-guard.mjs --mode staged (OK, 1 staged) → MISSIOND_TASK_CONTRACT=... git commit -m \"feat(tasks): verify task runner batches\" (commit fe70a5fcad04) → verify-task-contract.mjs (OK against fe70a5fcad04). All append-only ledger updates: shared-memory wave28-05-claim-001 (seq 8) before staging + wave28-05-completion-001 (seq 10) after verify; session-trace wave28-05-trace-start-001 (seq 11) before reading background + wave28-05-trace-commit-001 (seq 13, with commit hash) + wave28-05-trace-complete-001 (seq 14). Both ledgers re-validated after each append.

     Constraints honored: NO Rust / SQL / Cargo edits. Did not touch crates/**, .missiond/v2/**, .missiond/router/**, schemas (task-contract / task-runner-manifest), .missiond/tasks/wave27/**, any wave28-*.lisp other than session-trace + shared-memory (both are session-trace-writable / claim-allowed and explicitly NOT in :must-not-touch), .missiond/tasks/wave28/dispatch-plan.lisp, .missiond/claudecode/**, sibling scripts (check-task-runner-manifest.mjs, plan-task-runner.mjs, render-wave-briefs.mjs, render-claudecode-task.mjs). Did not modify scripts/verify-task-run.mjs (zero diff). Did not git add . / git push / --no-verify / --amend / --force.

     Parallel coordination: wave28-04 (mission_plan task-runner dry-run surface) ran concurrently in dispatch_group C against crates/missiond-{daemon,mcp}/src/handlers,tools/knowledge/plan.rs — zero file overlap with wave28-05's scripts/verify-task-runner-batch.mjs. Both agents appended to the same shared-memory + session-trace ledgers (read fresh, append-only).")
