;; Wave 29 / Task 05 — Verification receipt schema v0.
;; Schema: missiond.report-contract.v1
;; Source: .missiond/tasks/wave29/wave29-05-verification-receipt-schema-v0.lisp

(report wave29-05-verification-receipt-schema-v0
  :schema "missiond.report-contract.v1"
  :task_id "wave29-05-verification-receipt-schema-v0"
  :status done
  :commit_hash "ed7940f9d572"
  :files_changed
    [".missiond/tasks/schema/verification-receipt-v1.lisp"
     "scripts/check-verification-receipt.mjs"
     "scripts/verify-task-runner-batch.mjs"]

  :acceptance_results
    [(:command "node scripts/check-verification-receipt.mjs --dry-fixture"
      :exit_code 0
      :ok true
      :notes "verification-receipt fixtures OK (16 structural + 7 reuse-helper, 16 categories): pass / pass-start-finish / pass-multi (multi-receipt container) / pass-non-zero-exit (non-zero exit_code is STRUCTURALLY valid; reuse helper rejects it semantically) / fail-empty-command / fail-bad-duration (negative :duration_ms) / fail-bad-exit-code (non-integer) / fail-bad-tier (out-of-enum) / fail-bad-commit (malformed hex) / fail-bad-path (×2: absolute + traversal) / fail-stale-wave-task (task_id does not start with wave prefix) / fail-duplicate-id (same receipt_id twice in container) / fail-missing-required (task_id absent) / fail-missing-timing (need :duration_ms OR :started_at+:finished_at) / fail-bad-iso-8601 (started_at not ISO-8601) PLUS 7 reuse-helper checks pinning all four conservative reuse rules (exact commit / hex-prefix-agree commit / full-covers-smoke / local-CANNOT-cover-smoke / non-zero-exit / command-mismatch / unrelated-commit).")
     (:command "node scripts/verify-task-runner-batch.mjs --dry-fixture"
      :exit_code 0
      :ok true
      :notes "task-runner-batch verify fixtures OK (16 fixtures = 12 baseline + 4 wave29-05). Baseline fixtures unchanged: all-green / missing-report / missing-memory / commit-hash-mismatch / archive+backfill-pseudo-skipped / productive_only=false / determinism / contract-message-mismatch / wave28-06-loop-smoke / wave29-04 lineage ×3 (final-cite / worker-cite / outside-lineage). New wave29-05 cases: matching-receipt → reusable_count=1 (full verification still runs) / stale-commit-receipt → reusable_count=0 (verification still all_green) / no-receipts → receipt_coverage key absent (backward-compat byte-identical with baseline shape) / full-tier-receipt covers local-tier query → reusable_count=1. Plus an extra in-loop assertion that re-runs the all-green baseline WITHOUT receipts and confirms `Object.prototype.hasOwnProperty.call(baseline, 'receipt_coverage') === false` so the field is genuinely absent (not an empty array sneaking in).")
     (:command "node scripts/check-task-contract.mjs --all"
      :exit_code 0
      :ok true
      :notes "task-contract check OK (105 tasks) — all wave22..wave29 task contracts including this one parse and pass shape / scope / must-not-touch / acceptance / commit-policy validation. No regressions vs the wave29-04 baseline.")
     (:command "node scripts/check-task-report.mjs --all"
      :exit_code 0
      :ok true
      :notes "task-report check OK (73 reports) — all wave22..wave29 reports validate clean, including the new wave29-03/06 reports that landed in parallel during this task. wave29-04 hardening (lineage :kind enum + final-hash drift + extended hex format) stays green byte-identically.")
     (:command "git diff --check -- .missiond/tasks/schema/verification-receipt-v1.lisp scripts/check-verification-receipt.mjs scripts/verify-task-runner-batch.mjs"
      :exit_code 0
      :ok true
      :notes "no whitespace errors on any of the 3 staged paths; trailing-newline / tab-stop hygiene clean.")
     (:command "node scripts/check-missiond-hooks.mjs --json"
      :exit_code 0
      :ok true
      :notes "preflight OK; core.hooksPath aligned to .githooks; .githooks/pre-commit exists and is executable; no install required.")
     (:command "node scripts/task-scope-guard.mjs --task .missiond/tasks/wave29/wave29-05-verification-receipt-schema-v0.lisp --mode staged"
      :exit_code 0
      :ok true
      :notes "task-scope-guard staged OK: wave29-05-verification-receipt-schema-v0 (3 staged file(s)) — all 3 staged paths inside :write-scope; zero matches against :must-not-touch (crates/** .missiond/v2/** .missiond/router/** .missiond/tasks/schema/{task-contract,report-contract,context-atlas,pattern-card}-v1.lisp .missiond/tasks/wave28/** .missiond/tasks/wave29/wave29-*.lisp .missiond/tasks/wave29/manifest.lisp .missiond/tasks/wave29/dispatch-plan.lisp .missiond/claudecode/** scripts/{check-context-atlas,check-pattern-card,check-task-report,verify-task-run,prepare-task-runner-wave,render-wave-briefs,plan-task-runner}.mjs). Note: scripts/plan-task-runner.mjs was modified-but-unstaged in the working tree by the parallel wave29-06 worker; it never appeared in the staged set so the guard correctly accepted only my 3 paths.")
     (:command "node scripts/verify-task-contract.mjs .missiond/tasks/wave29/wave29-05-verification-receipt-schema-v0.lisp"
      :exit_code 0
      :ok true
      :notes "task-contract verify OK: wave29-05-verification-receipt-schema-v0 against ed7940f9d572 — commit hash exists; commit message matches `feat(tasks): add verification receipt checks` per contract; changed_files ⊆ write-scope (write-scope-only); changed_files ∩ must-not-touch = ∅; acceptance commands present in contract.")]

  :scope_deviations []

  :trace_refs [wave29-05-trace-start-001 wave29-05-trace-read-001 wave29-05-trace-commit-001 wave29-05-trace-complete-001]

  :major_decisions
    [(:decision "Schema accepts BOTH the multi-receipt container shape (verification-receipt-set <set-id> ...) and the per-file single-receipt shape (verification-receipt <receipt-id> ...)."
      :rationale "Mirrors the wave29-02 pattern-card schema decision so a wave's receipts batch into one file (recommended) while ad-hoc per-task receipt capture remains valid. One validator covers both shapes; receipt-body validation rules are identical across both shapes — only the header (which carries :schema / :version / :wave / :generated_at) lives on the container vs the single-receipt form.")
     (:decision "Timing is REQUIRED-OR-DERIVED: each receipt MUST carry EITHER :duration_ms OR (:started_at + :finished_at) (or both)."
      :rationale "Brief allowed either form; both shapes are commonly emitted (Date.now()-style harnesses produce duration_ms; orchestrator-recorded events naturally produce start/finish ISO timestamps). Allowing BOTH covers both producers without forcing a translation layer. The checker does NOT cross-check :duration_ms against (finished_at - started_at) — they are independently advisory; structural validation is the schema's job, not arithmetic reconciliation. :started_at without :finished_at (or vice versa) is rejected so the timing pair stays consistent.")
     (:decision "Stale wave/task mismatch is a HARD REJECT: receipt :task_id MUST start with :wave + '-'."
      :rationale "Brief explicitly required defence against accidentally-mixed receipt files (e.g. a wave28 receipt pasted into a wave29 receipts file). Catching this at the structural-validation layer prevents the planner from ever seeing a misplaced receipt. Cheap check; high signal.")
     (:decision "Conservative reuse rules consolidated into ONE exported helper `isReceiptReusable(receipt, query)`. ALL FOUR rules MUST hold for true; ANY failure → false."
      :rationale "Brief explicitly named this helper as the canonical reuse rule that wave29-06 / wave29-07 / future tasks call instead of re-implementing. Centralising it kills the risk of subtle drift across planners. The four rules: (1) hex-prefix-agree commit_hash (mirrors wave29-04 lineage helper); (2) command exact match after .trim() (no fuzzy matching — receipts cache a SPECIFIC command line); (3) :exit_code === 0 (any non-zero invalidates evidence); (4) tier covering full > smoke > local (asymmetric — local CANNOT cover smoke). Documented in code comments as load-bearing — receipts are CACHE rules NOT verification rules; the orchestrator MUST still verify the commit through the normal path.")
     (:decision "verify-task-runner-batch --receipts is OPTIONAL and additive. When omitted, the JSON output is BYTE-IDENTICAL to the wave28-05 + wave29-04 baseline (no receipt_coverage key emitted)."
      :rationale "Backward compat is non-negotiable per brief. Implementation: aggregateResults takes a 4th optional `receipts` arg; only when non-null does it inject `receipt_coverage` into the result object. verifyManifest's signature gained a default `receipts = null`. The 12 baseline fixtures continue to call verifyManifest with NO receipts and stay byte-identical. An additional explicit assertion at the end of runFixtures re-runs the all-green baseline without receipts and confirms `Object.prototype.hasOwnProperty.call(baseline, 'receipt_coverage') === false` so we catch any future drift toward an empty-array default.")
     (:decision "When --receipts is supplied, the CLI HARD-fails on malformed receipts (uses validateReceiptObject). Receipts are advisory but the cache must be structurally trustworthy."
      :rationale "Silently ingesting a broken evidence cache would let a typo (e.g. negative duration_ms) infect downstream planners. The hard failure surfaces the typo immediately. The verifier ALWAYS runs the full task-contract / report / memory / commit verification regardless — receipts only affect the receipt_coverage hint, never the aggregate_status.")
     (:decision "Receipt :id resolved from second form (canonical) OR :receipt_id keyword (alternative); both present and disagreeing is a structural error. When neither is supplied the checker derives a deterministic id from {wave}-{task_id}-{commit[:7]}-{tier} so the duplicate-id pass still works."
      :rationale "Mirrors the wave29-02 pattern-card id-resolution policy. Derivation gives the duplicate-id detector something to compare against even for unnamed receipts; the derived id still has to match the same kebab pattern.")
     (:decision "ISO-8601 validation uses both a regex (YYYY-MM-DDTHH:MM:SS(.fff)?(Z|±HH:MM)) AND Date.parse for finiteness (rejects e.g. February 31)."
      :rationale "Regex alone passes 2026-02-31; Date.parse alone is too lenient (accepts 'April 28 2026'). Combining both gets us the precise ISO-8601 shape AND calendar validity in 3 lines.")
     (:decision "main() gated on `import.meta.url === pathToFileURL(process.argv[1]).href` so wave29-06 / wave29-07 / verify-task-runner-batch can import named exports without triggering CLI side-effects (mirrors check-pattern-card.mjs and verify-task-runner-batch.mjs)."
      :rationale "Standard MissionD pattern. Importers should not need to monkey-patch process.argv or worry about top-level CLI parsing firing.")]

  :time_sinks
    [(:label "Designing the schema to make timing required-or-derived (either :duration_ms OR :started_at+:finished_at OR both) without forcing a translation layer"
      :notes "Brief said pick one and document or BOTH allowed but at least one required. Picked BOTH allowed because the producers naturally emit one or the other depending on harness. The structural rule reduces to: if hasStarted XOR hasFinished → reject (asymmetric); if !hasStarted && !hasFinished && !hasDuration → reject (no timing); otherwise accept. Three lines of structural validation, zero arithmetic reconciliation.")
     (:label "Authoring 16 structural fixtures + 7 reuse-helper fixtures across 16 categories (target was 12-16)"
      :notes "Targeted 12-16, landed at 23 total cases across 16 categories. Critical coverage: every required field has a fail-missing or fail-malformed case; the stale-wave-task-mismatch defence is exercised explicitly; the 4 conservative reuse rules each get a dedicated reuse-helper fixture (rule 4 is the most subtle — local CANNOT cover smoke); the pass-non-zero-exit case explicitly demonstrates that structural validation accepts non-zero exit while the reuse helper rejects it.")
     (:label "Wiring backward-compat byte-identical guarantees into verify-task-runner-batch.mjs"
      :notes "Three layers: (1) aggregateResults only injects receipt_coverage when receipts is non-null; (2) verifyManifest defaults receipts to null; (3) an explicit in-loop assertion re-runs the all-green baseline without receipts and inspects the result object's keys for the absence of receipt_coverage. The 12 baseline fixtures (wave28-05 9 + wave29-04 3) all continue to call verifyManifest WITHOUT receipts and pass byte-identically.")
     (:label "Authoring computeReceiptCoverage with stable byte-deterministic output"
      :notes "Coverage rows are sorted by task_id ascending, decisions sorted by receipt_id ascending. The reason field uses describeReuseFailure to give the planner a human-readable hint about WHY a receipt was not reusable (rule 1/2/3/4) so debugging stays easy.")]

  :unexpected_work
    [(:summary "Pre-commit lint heads-up from coordinator: TS6133 unused `opts` parameter on validateReceiptForm. Originally added speculatively for future allowHeaderFields configuration but the actual code path derives that decision from `isSingle` (which is already determined by `head(form)`). Fix: dropped the parameter from the function signature AND from the two call sites in validateForms / validateReceiptContainer. The header-vs-no-header field-set selection still works correctly (still uses `isSingle` directly). All 16 + 7 fixtures stay green after the cleanup.")
     (:summary "Coordinator heads-up about TWO unused imports `readVerificationReceiptFile` + `validateReceiptObject` in verify-task-runner-batch.mjs was a stale lint snapshot — both ARE called in the runCli --receipts path I had just added (lines 783 + 790). Confirmed by grep before the commit landed; no code change needed. Documented here so future audits see the rationale.")
     (:summary "scripts/plan-task-runner.mjs appeared as `M ` (staged in index) because the parallel wave29-06 worker had already staged its file in the shared workspace. The first scope-guard run failed with `staged files inside :must-not-touch: scripts/plan-task-runner.mjs`. Fix: `git reset HEAD scripts/plan-task-runner.mjs` to unstage that one file (which left it as ` M` modified-but-unstaged so wave29-06 still has its work intact on disk for its own commit), then re-staged my 3 declared paths. Scope guard then accepted the 3-file set cleanly. This is a Group B parallel-execution race condition — both wave29-05 and wave29-06 share the staging area; the fix is read-fresh + selective unstage rather than touching wave29-06's file content.")]

  :recommended_backend "claudecode"
  :router_confidence "high"
  :router_policy_path ".missiond/router/router-policy-v1.lisp"
  :router_dry_run_only true
  :router_applied false
  :router_reasons
    ["Dispatch strategy fresh-code-alignment + owner claudecode → matches r-fresh-code-alignment-to-claudecode in router-policy-v1 (priority 100, single matched rule)."
     "Workstation surface (NEW Lisp schema doc + NEW Node.js read-only checker + surgical Edit to existing read-only Node verifier; no Rust / SQL / cargo) is the canonical claudecode beat — pure file reads, no network / LLM call required."
     "Router output is recorded for telemetry only; runtime dispatch unchanged (claudecode is the live default and remained the live default for this task)."]
  :router_trace_index_path ".missiond/router/trace-index-v1.lisp"

  :router_backend_readiness_status "current-default"
  :router_backend_runtime_allowed true
  :router_apply_eligible false
  :router_apply_blockers
    ["current-default is the live runtime today but explicit runtime-ready opt-in is required upstream before this task's dispatch handoff would mark a descriptor as apply-eligible (wave27-01 eligibility-gate intentionally REJECTS current-default → eligible)."]
  :router_backend_registry_path ".missiond/router/router-backend-registry-v1.lisp"

  :notes
    "wave29-05 ships:
     - .missiond/tasks/schema/verification-receipt-v1.lisp (Lisp schema doc; canonical missiond.verification-receipt.v1; documents container + single-receipt shapes, header contract, receipt contract, validation contract, checker contract, reuse-helper contract, and the four conservative reuse rules verbatim).
     - scripts/check-verification-receipt.mjs (read-only Node checker; reuses scripts/lib/missiond_lisp.mjs; --json --stdin --dry-fixture flags; never shells out / never touches git / network / LLM).
     - scripts/verify-task-runner-batch.mjs (surgical Edit: added optional --receipts flag, computeReceiptCoverage helper, receipt_coverage field in aggregate output ONLY when receipts supplied, plus 4 new wave29-05 fixtures; 12 wave28-05 + wave29-04 baseline fixtures stay BYTE-IDENTICAL).

     Schema head: (verification-receipt-schema missiond.verification-receipt.v1 ...).
     Top-level forms accepted: (verification-receipt-set <set-id> ...) container OR (verification-receipt <receipt-id> ...) single-record.
     Required receipt fields: :wave :task_id :commit_hash :command :exit_code :tier (PLUS at least one of :duration_ms or :started_at+:finished_at).
     Optional receipt fields: :receipt_id :files :notes (and the alternate timing form).

     The four conservative reuse rules (encoded verbatim in `isReceiptReusable`):
       1. receipt.exit_code === 0                          (rule 3 fail → false)
       2. receipt.commit_hash agrees with query.commit_hash via hex-prefix-agree (longer starts with shorter; shorter >= 7 hex chars; mirrors wave29-04 lineage helper) (rule 1 fail → false)
       3. receipt.command.trim() === query.command.trim() (no fuzzy / argv-reorder matching — receipts cache a SPECIFIC command line) (rule 2 fail → false)
       4. tier covering: full covers {local, smoke, full}; smoke covers {local, smoke}; local covers {local} ASYMMETRIC (rule 4 fail → false)
     ANY failure → reuse=false. Receipts are CACHE rules NOT verification rules; documented as load-bearing in code comments. Orchestrators that decide to skip a re-run based on isReceiptReusable MUST still verify task contract / report / memory completion / git commit through the normal path.

     Named exports for downstream tooling (10 total):
       SCHEMA                        = literal 'missiond.verification-receipt.v1'
       SET_HEAD                      = literal 'verification-receipt-set'
       SINGLE_HEAD                   = literal 'verification-receipt'
       RECEIPT_HEAD                  = literal 'receipt'
       TIERS                         = Object.freeze(['local', 'smoke', 'full'])
       projectReceipt(form)          = parsed-form -> projected receipt object
       readVerificationReceiptFile   = on-disk -> projected receipt array (flattens (verification-receipt-set ...) containers)
       validateReceiptObject(obj)    = projected receipt -> string[] of error messages (empty == valid)
       isReceiptReusable             = canonical conservative reuse helper (4 rules)
       computeReceiptCoverage        = exported from verify-task-runner-batch.mjs (per-task coverage rows, sorted, byte-stable)

     verify-task-runner-batch.mjs integration:
       - New CLI flag --receipts <file.lisp> (optional). When supplied, reads via readVerificationReceiptFile, validates via validateReceiptObject (HARD-fails on malformed receipts), and feeds them to computeReceiptCoverage.
       - New aggregateResults(manifestPath, manifest, perNodeResults, receipts) signature (4th arg defaults undefined → no behavioural change without --receipts).
       - New verifyManifest({ manifestPath, manifest, loaders, receipts = null }) signature.
       - New computeReceiptCoverage(manifest, perNodeResults, receipts) export — per-task receipt count + reuse decisions; sorted byte-stably (task_id ascending; receipt_id ascending within task).
       - New `receipt_coverage` field in JSON output, emitted ONLY when receipts is non-null. Backward compat ENFORCED by an explicit in-loop assertion that re-runs the all-green baseline WITHOUT receipts and confirms Object.hasOwn(result, 'receipt_coverage') === false.
       - Verifier MUST still verify task contract / report / memory completion / git commit even when receipts present. Documented in code comments as load-bearing.

     Dry-fixture totals:
       check-verification-receipt.mjs --dry-fixture: 16 structural + 7 reuse-helper = 23 cases across 16 categories.
       verify-task-runner-batch.mjs --dry-fixture: 16 fixtures (12 baseline byte-identical + 4 wave29-05).
       Categories: pass / pass-start-finish / pass-multi / pass-non-zero-exit / fail-empty-command / fail-bad-duration / fail-bad-exit-code / fail-bad-tier / fail-bad-commit / fail-bad-path (×2) / fail-stale-wave-task / fail-duplicate-id / fail-missing-required / fail-missing-timing / fail-bad-iso-8601 / reuse-helper.

     Hard guarantees (verified by grep): NO child_process / spawn / fetch / http / https / exec / git / openai / anthropic in scripts/check-verification-receipt.mjs (pure file reads via the missiond_lisp reader). NO new git mutation in the verify-task-runner-batch.mjs edits — the only git surface remains the read-only readCommit helper.

     main() gated on `import.meta.url === pathToFileURL(process.argv[1]).href` so importers do not accidentally trigger CLI side-effects.

     Pre-commit pipeline:
       check-verification-receipt.mjs --dry-fixture (exit=0, 23/23)
       -> verify-task-runner-batch.mjs --dry-fixture (exit=0, 16/16; 12 baseline byte-identical + 4 wave29-05)
       -> check-task-contract.mjs --all (exit=0, 105 tasks)
       -> check-task-report.mjs --all (exit=0, 73 reports)
       -> git diff --check (exit=0)
       -> check-missiond-hooks.mjs --json (preflight aligned)
       -> git add (only the 3 declared paths staged; one accidental wave29-06 staging unstaged via git reset HEAD without touching its content)
       -> task-scope-guard.mjs --mode staged (OK, 3 staged)
       -> MISSIOND_TASK_CONTRACT=... git commit -m 'feat(tasks): add verification receipt checks' (worker commit ed7940f9d572)
       -> verify-task-contract.mjs against final commit ed7940f9d572 (exit=0).

     Append-only ledger updates:
       shared-memory: wave29-05-claim-001 (seq 10) before staging + wave29-05-completion-001 (seq 11+) after verify.
       session-trace: wave29-05-trace-start-001 (seq 19) + wave29-05-trace-read-001 (seq 20) before reading background + wave29-05-trace-commit-001 (commit hash) + wave29-05-trace-complete-001 (report path) after.
       Both ledgers re-read fresh before each append because the parallel wave29-03 + wave29-06 agents appended their own claim/start/completion entries between this task's claim and completion.

     Constraints honored: NO Rust / SQL / Cargo edits. Did not touch crates/**, .missiond/v2/**, .missiond/router/**, .missiond/tasks/schema/{task-contract,report-contract,context-atlas,pattern-card}-v1.lisp, .missiond/tasks/wave28/**, .missiond/tasks/wave29/wave29-*.lisp, .missiond/tasks/wave29/manifest.lisp, .missiond/tasks/wave29/dispatch-plan.lisp, .missiond/claudecode/**, scripts/check-context-atlas.mjs, scripts/check-pattern-card.mjs, scripts/check-task-report.mjs, scripts/verify-task-run.mjs, scripts/prepare-task-runner-wave.mjs, scripts/render-wave-briefs.mjs, scripts/plan-task-runner.mjs (the parallel wave29-06 worker's modified-on-disk version stayed untouched; only my 3 declared paths landed in the commit). Did not git add . / git push / --no-verify / --amend / --force.")
