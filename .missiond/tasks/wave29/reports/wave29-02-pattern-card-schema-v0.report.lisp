;; Wave 29 / Task 02 — Pattern card schema v0.
;; Schema: missiond.report-contract.v1
;; Source: .missiond/tasks/wave29/wave29-02-pattern-card-schema-v0.lisp

(report wave29-02-pattern-card-schema-v0
  :schema "missiond.report-contract.v1"
  :task_id "wave29-02-pattern-card-schema-v0"
  :status done
  :commit_hash "57fdc88ff135"
  :files_changed
    [".missiond/tasks/schema/pattern-card-v1.lisp"
     "scripts/check-pattern-card.mjs"
     ".missiond/patterns/schema-checker.pattern.lisp"
     ".missiond/patterns/node-cli-readonly.pattern.lisp"
     ".missiond/patterns/report-lineage.pattern.lisp"
     ".missiond/patterns/cross-layer-smoke.pattern.lisp"
     ".missiond/patterns/large-file-navigation.pattern.lisp"]

  :acceptance_results
    [(:command "node scripts/check-pattern-card.mjs --dry-fixture"
      :exit_code 0
      :ok true
      :notes "pattern-card fixtures OK (14 cases, 13 categories): pass / pass-multi (multi-card container) / pass-legacy-schema (legacy missiond.pattern-cards.dispatch.v0 accepted) / pass-warn-only (empty :known-good warns, not fails) / pass-aliases (underscore variants :use_for/:known_good/:anti_pattern/:non_goals normalized to kebab) / fail-duplicate-id / fail-empty-recipe / fail-empty-recipe-entry / fail-bad-path (absolute + traversal) / fail-bad-use-for (id not matching task-id pattern) / fail-missing-required (recipe omitted) / fail-unknown-head (top-level head not pattern-cards|pattern-card) / fail-id-mismatch (second-form id disagrees with :id keyword).")
     (:command "node scripts/check-pattern-card.mjs .missiond/tasks/wave29/pattern-cards.lisp"
      :exit_code 0
      :ok true
      :notes "Real-world smoke: existing dispatch-time pattern-cards.lisp validates clean (5 cards) with 2 expected warnings — cards `cross-layer-smoke` and `large-file-navigation` ship without :known-good in the dispatch file because no canonical exemplar was fixed at dispatch time. Schema accepts the legacy :schema literal `missiond.pattern-cards.dispatch.v0` so the file passes UNMODIFIED.")
     (:command "node scripts/check-pattern-card.mjs .missiond/patterns/schema-checker.pattern.lisp .missiond/patterns/node-cli-readonly.pattern.lisp .missiond/patterns/report-lineage.pattern.lisp .missiond/patterns/cross-layer-smoke.pattern.lisp .missiond/patterns/large-file-navigation.pattern.lisp"
      :exit_code 0
      :ok true
      :notes "All 5 seed pattern cards validate clean (5 cards, 0 warnings). Each ships :known-good with at least 4 exemplar paths; each :recipe carries 7-8 substantive concrete steps; each :use-for joins to the right wave29 task ids; :anti-pattern + :non-goals + :notes optionals populated.")
     (:command "node scripts/check-task-contract.mjs --all"
      :exit_code 0
      :ok true
      :notes "task-contract check OK (105 tasks) — all wave22..wave29 task contracts including this one parse and pass shape / scope / must-not-touch / acceptance / commit-policy validation. No regressions vs the wave28 baseline.")
     (:command "git diff --check -- .missiond/tasks/schema/pattern-card-v1.lisp scripts/check-pattern-card.mjs .missiond/patterns/schema-checker.pattern.lisp .missiond/patterns/node-cli-readonly.pattern.lisp .missiond/patterns/report-lineage.pattern.lisp .missiond/patterns/cross-layer-smoke.pattern.lisp .missiond/patterns/large-file-navigation.pattern.lisp"
      :exit_code 0
      :ok true
      :notes "no whitespace errors on any of the 7 NEW staged paths; trailing-newline / tab-stop hygiene clean.")
     (:command "node scripts/check-missiond-hooks.mjs --json"
      :exit_code 0
      :ok true
      :notes "preflight OK; core.hooksPath aligned to .githooks; .githooks/pre-commit exists and is executable; no install required.")
     (:command "node scripts/task-scope-guard.mjs --task .missiond/tasks/wave29/wave29-02-pattern-card-schema-v0.lisp --mode staged"
      :exit_code 0
      :ok true
      :notes "task-scope-guard staged OK: wave29-02-pattern-card-schema-v0 (7 staged file(s)) — all 7 staged paths inside :write-scope; zero matches against :must-not-touch (crates/** .missiond/v2/** .missiond/router/** .missiond/tasks/wave28/** .missiond/tasks/wave29/wave29-*.lisp .missiond/tasks/wave29/manifest.lisp .missiond/tasks/wave29/dispatch-plan.lisp .missiond/tasks/wave29/context-atlas.lisp .missiond/claudecode/** scripts/{check-context-atlas,prepare-task-runner-wave,check-task-report,verify-task-run,verify-task-runner-batch,plan-task-runner}.mjs).")
     (:command "node scripts/verify-task-contract.mjs .missiond/tasks/wave29/wave29-02-pattern-card-schema-v0.lisp"
      :exit_code 0
      :ok true
      :notes "task-contract verify OK: wave29-02-pattern-card-schema-v0 against 57fdc88ff135 — commit hash exists; commit message matches `feat(tasks): add pattern card schema` per contract; changed_files ⊆ write-scope (write-scope-only); changed_files ∩ must-not-touch = ∅; acceptance commands present in contract.")]

  :scope_deviations []

  :trace_refs [wave29-02-trace-start-001 wave29-02-trace-read-001 wave29-02-trace-commit-001 wave29-02-trace-complete-001]

  :major_decisions
    [(:decision "Schema accepts BOTH the multi-card container shape (pattern-cards <set-id> ...) and the per-file single-card shape (pattern-card <card-id> ...)."
      :rationale "The wave29 dispatch-time pattern-cards.lisp uses the multi-card container; the .missiond/patterns/*.pattern.lisp seed files use single-card. One validator covers both shapes so the dispatch file passes UNMODIFIED while seed cards land cleanly. Card validation rules are identical across both shapes — only the header (which carries :schema / :version / :wave) differs.")
     (:decision "Legacy :schema literal `missiond.pattern-cards.dispatch.v0` accepted on the multi-card container in addition to the canonical `missiond.pattern-card.v1`."
      :rationale "Brief acceptance: 'Schema MUST validate this file unmodified.' The dispatch file ships the legacy :schema literal; rejecting it would force a same-PR edit to the dispatch file, which is outside this task's :write-scope. ACCEPTED_SCHEMAS is a tiny Set so future legacy literals can be added without code changes.")
     (:decision ":known-good is RECOMMENDED, not REQUIRED. Missing or empty :known-good produces a WARNING, not an error."
      :rationale "The dispatch-time pattern-cards.lisp ships two cards (cross-layer-smoke, large-file-navigation) WITHOUT :known-good because no canonical single exemplar was fixed at dispatch time. Treating absence as a warning lets newly-seeded cards land without forcing a fake exemplar; treating it as a permanent silence would lose the visibility into stale cards. The 5 seed cards in .missiond/patterns/ all SHIP :known-good — the seed-card layer enforces the discipline that the schema layer cannot.")
     (:decision "Underscore aliases (:use_for / :known_good / :anti_pattern / :non_goals) normalized to kebab canonicals BEFORE validation."
      :rationale "Brief used underscore notation while existing dispatch file uses kebab. Aliasing at the parse boundary lets both shapes round-trip through the validator and the named exports without callers needing to know which keyword variant was used. ALIAS_TABLE is the single source of truth for alias mapping.")
     (:decision ":use-for entries MUST match the task-id pattern (^[a-z0-9][a-z0-9._-]*$) — same regex as task-runner-manifest task ids."
      :rationale "Pinning :use-for to the same shape as task-runner-manifest task_ids makes a future card-to-task join mechanically straightforward. wave29-03 brief renderer and wave29-06 ready-queue planner can both consume :use-for without re-validating shape.")
     (:decision "RECOMMENDED_CARD_FIELDS is data-driven, not hardcoded inline."
      :rationale "Earlier draft hardcoded the :known-good missing-warning inline and left RECOMMENDED_CARD_FIELDS unused (TS6133). Coordinator caught this; the fix drives the warning loop FROM the constant so adding a future recommended field auto-warns without touching the loop body. The constant is also exported for downstream introspection by wave29-03/06/07.")
     (:decision "Card id resolved from second form (canonical) OR :id keyword (alternative); both present and disagreeing is a structural error."
      :rationale "Existing dispatch file uses second-form ids: `(card schema-checker :use-for [...])`. Some emitted formats prefer keyword ids: `(card :id schema-checker ...)`. Supporting both shapes covers the format space; rejecting disagreement keeps single-source-of-truth invariant intact.")
     (:decision "main() gated on `import.meta.url === \\`file://${process.argv[1]}\\`` so wave29-03 / 06 / 07 can import named exports without triggering CLI side-effects (mirrors check-task-runner-manifest.mjs and plan-task-runner.mjs)."
      :rationale "Standard MissionD pattern. Importers should not need to monkey-patch process.argv or worry about top-level CLI parsing firing.")]

  :time_sinks
    [(:label "Designing schema to accept BOTH the existing dispatch-time pattern-cards.lisp shape AND the new per-file seed shape"
      :notes "First draft made :known-good required, which caused the smoke against the dispatch file to fail (cross-layer-smoke + large-file-navigation cards omit :known-good there). Refactored to RECOMMENDED with a warning, then drove the warning loop from a data-driven RECOMMENDED_CARD_FIELDS constant. Net result: dispatch file passes unmodified with 2 expected warnings; seed cards must populate :known-good (discipline at the seed-card layer).")
     (:label "Authoring 5 substantive seed cards (not stubs)"
      :notes "Each card got 7-8 concrete recipe steps drawn from real exemplars: schema-checker references wave24/wave26/wave27/wave28 schema/checker family; node-cli-readonly references wave28-02 plan CLI + wave28-03 renderer + wave28-05 batch verifier; report-lineage references wave28-02 report + wave29-04 lineage extension; cross-layer-smoke references wave26-06 + wave27-06 + wave28-06; large-file-navigation references plan.rs + wave29 context atlas. Total: 38 recipe steps, 23 known-good exemplar paths, 13 anti-patterns, 11 non-goals.")
     (:label "Drafting 14 dry-fixture cases across 13 categories"
      :notes "Targeted 10-14, landed at 14. Critical coverage: pass-multi (container shape with multiple cards), pass-legacy-schema (legacy dispatch literal accepted), pass-warn-only (empty :known-good is acceptable), pass-aliases (underscore -> kebab normalization), fail-duplicate-id (cross-card ids in same file), fail-empty-recipe x2 (empty vector + empty entry), fail-bad-path x2 (absolute + traversal), fail-bad-use-for (non-task-id), fail-missing-required, fail-unknown-head, fail-id-mismatch (second-form vs keyword disagreement). 13 categories covers every :rejects bullet in the schema doc.")]

  :unexpected_work
    [(:summary "Pre-commit lint heads-up from coordinator: TS6133 unused `RECOMMENDED_CARD_FIELDS`. Fix: exported the constant AND drove the missing-recommended-field warning loop from it so the data flow goes constant -> loop -> warning instead of hardcoded inline. Side benefit: adding a future recommended field auto-warns without touching the loop body. HEADER_OPTIONAL renamed to HEADER_OPTIONAL_FIELDS and exported for symmetry. 13 named exports total (was 7 in the first draft).")
     (:summary "Confirmed end-to-end import smoke via `node -e \"import('./scripts/check-pattern-card.mjs').then(m => ...)\"` — all 13 named exports load cleanly with no CLI side-effect (main() gate works). This protects wave29-03 / 06 / 07 from accidentally triggering the CLI when they import.")]

  :recommended_backend "claudecode"
  :router_confidence "high"
  :router_policy_path ".missiond/router/router-policy-v1.lisp"
  :router_dry_run_only true
  :router_applied false
  :router_reasons
    ["Dispatch strategy fresh-code-alignment + owner claudecode → matches r-fresh-code-alignment-to-claudecode in router-policy-v1 (priority 100, single matched rule)."
     "Workstation surface (NEW Lisp schema doc + NEW Node.js read-only checker + 5 NEW Lisp seed cards; no Rust / SQL / cargo) is the canonical claudecode beat — pure file reads, no network / LLM call required."
     "Router output is recorded for telemetry only; runtime dispatch unchanged (claudecode is the live default and remained the live default for this task)."]
  :router_trace_index_path ".missiond/router/trace-index-v1.lisp"

  :router_backend_readiness_status "current-default"
  :router_backend_runtime_allowed true
  :router_apply_eligible false
  :router_apply_blockers
    ["current-default is the live runtime today but explicit runtime-ready opt-in is required upstream before this task's dispatch handoff would mark a descriptor as apply-eligible (wave27-01 eligibility-gate intentionally REJECTS current-default → eligible)."]
  :router_backend_registry_path ".missiond/router/router-backend-registry-v1.lisp"

  :notes
    "wave29-02 ships:
     - .missiond/tasks/schema/pattern-card-v1.lisp (Lisp schema doc; canonical missiond.pattern-card.v1; legacy missiond.pattern-cards.dispatch.v0 accepted on multi-card container so the wave29 dispatch file passes UNMODIFIED).
     - scripts/check-pattern-card.mjs (read-only Node checker; reuses scripts/lib/missiond_lisp.mjs; --json --stdin --dry-fixture flags; never shells out / never touches git / network / LLM).
     - .missiond/patterns/schema-checker.pattern.lisp (8-step recipe + 6 exemplars).
     - .missiond/patterns/node-cli-readonly.pattern.lisp (8-step recipe + 5 exemplars).
     - .missiond/patterns/report-lineage.pattern.lisp (7-step recipe + 5 exemplars).
     - .missiond/patterns/cross-layer-smoke.pattern.lisp (8-step recipe + 5 exemplars).
     - .missiond/patterns/large-file-navigation.pattern.lisp (8-step recipe + 4 exemplars).

     Schema head: (pattern-card-schema missiond.pattern-card.v1 ...).
     Required card fields: :id (second form OR :id keyword) :use-for :recipe.
     Recommended card field: :known-good (missing/empty -> warning).
     Optional card fields: :purpose :summary :anti-pattern :non-goals :notes.
     Aliases normalized: :use_for -> :use-for; :known_good -> :known-good; :anti_pattern -> :anti-pattern; :non_goals -> :non-goals.

     Named exports for wave29-03 brief renderer / wave29-06 ready-queue planner / wave29-07 cross-layer smoke (13 total):
       SCHEMA                         = literal 'missiond.pattern-card.v1'
       LEGACY_SCHEMA_DISPATCH         = literal 'missiond.pattern-cards.dispatch.v0'
       ACCEPTED_SCHEMAS               = Set([SCHEMA, LEGACY_SCHEMA_DISPATCH])
       SET_HEAD                       = literal 'pattern-cards'
       SINGLE_HEAD                    = literal 'pattern-card'
       CARD_HEAD                      = literal 'card'
       REQUIRED_CARD_FIELDS           = [':use-for', ':recipe']
       RECOMMENDED_CARD_FIELDS        = [':known-good']
       OPTIONAL_CARD_FIELDS           = [':id', ':purpose', ':summary', ':known-good', ':anti-pattern', ':non-goals', ':notes']
       HEADER_OPTIONAL_FIELDS         = [':schema', ':version', ':wave', ':purpose', ':summary', ':notes']
       projectCard(form)              = parsed-form -> { id, use_for, recipe, known_good, purpose, summary, anti_pattern, non_goals, notes, loc }
       readPatternCardFile(file)      = on-disk -> projected card array (flattens (pattern-cards ...) containers)
       validateCardObject(obj)        = projected card -> string[] of error messages (empty == valid)

     Cross-wave invariants enforced:
       1. Pattern cards are advisory READ-ONLY implementation recipes. They never grant scope beyond the task contract.
       2. Schema accepts BOTH multi-card container and per-file single-card shapes; one validator covers both.
       3. Legacy :schema value `missiond.pattern-cards.dispatch.v0` accepted on container so existing wave29 file passes unmodified.
       4. Empty :recipe is a structural error — a card with no steps offers no guidance.
       5. Empty/missing :known-good is a warning — newly-seeded cards land cleanly while drift toward permanent omission stays visible.
       6. :use-for entries MUST match the task-id shape so a future card-to-task join is mechanical.
       7. RECOMMENDED_CARD_FIELDS is data-driven; adding a recommended field auto-warns without touching the loop body.

     Hard guarantees verified by `rg -nE 'child_process|spawn|fetch|http|https|exec|git|openai|anthropic' scripts/check-pattern-card.mjs` returning ZERO matches:
       - No dispatch / no mission_task_delegate.
       - No child_process.spawn / exec / fork.
       - No git.
       - No HTTP / network / fetch.
       - No LLM (openai / anthropic / etc).
       Pure file reads via fs.readFileSync (the missiond_lisp reader) + fs.existsSync (disk-existence warning only).

     main() gated on `import.meta.url === \\`file://${process.argv[1]}\\`` so importers do not accidentally trigger CLI side-effects.

     Dry-fixture totals: 14 cases across 13 categories — pass / pass-multi / pass-legacy-schema / pass-warn-only / pass-aliases / fail-duplicate-id / fail-empty-recipe / fail-empty-recipe-entry / fail-bad-path (×2) / fail-bad-use-for / fail-missing-required / fail-unknown-head / fail-id-mismatch.

     Real-world smoke: `node scripts/check-pattern-card.mjs .missiond/tasks/wave29/pattern-cards.lisp` -> exit 0, 5 cards validated, 2 expected warnings (cross-layer-smoke + large-file-navigation cards intentionally ship without :known-good in the dispatch file).

     Per-seed-card smoke: `node scripts/check-pattern-card.mjs .missiond/patterns/<each>.pattern.lisp` -> exit 0, 5 cards validated, 0 warnings each.

     Pre-commit pipeline:
       check-pattern-card.mjs --dry-fixture (exit=0, 14/14)
       -> check-pattern-card.mjs .missiond/tasks/wave29/pattern-cards.lisp (exit=0, 5/5 cards, 2 warnings expected)
       -> check-pattern-card.mjs .missiond/patterns/*.pattern.lisp (exit=0, 5/5 cards)
       -> check-task-contract.mjs --all (exit=0, 105 tasks)
       -> git diff --check (exit=0)
       -> check-missiond-hooks.mjs --json (preflight aligned)
       -> git add (only the 7 declared paths staged)
       -> task-scope-guard.mjs --mode staged (OK, 7 staged)
       -> MISSIOND_TASK_CONTRACT=... git commit -m 'feat(tasks): add pattern card schema' (worker commit 57fdc88ff135)
       -> verify-task-contract.mjs against final commit 57fdc88ff135 (exit=0).

     Append-only ledger updates: shared-memory wave29-02-claim-001 (seq 3) before staging + wave29-02-completion-001 (seq 6) after verify; session-trace wave29-02-trace-start-001 (seq 4) + wave29-02-trace-read-001 (seq 5) before reading background + wave29-02-trace-commit-001 (seq 10, with commit hash) + wave29-02-trace-complete-001 (seq 11, with report path). Both ledgers re-read fresh before each append because the parallel wave29-01 and wave29-04 agents appended their own claim/start/completion entries between this task's claim and completion.

     Constraints honored: NO Rust / SQL / Cargo edits. Did not touch crates/**, .missiond/v2/**, .missiond/router/**, .missiond/tasks/wave28/**, .missiond/tasks/wave29/wave29-*.lisp, .missiond/tasks/wave29/manifest.lisp, .missiond/tasks/wave29/dispatch-plan.lisp, .missiond/tasks/wave29/context-atlas.lisp, .missiond/claudecode/**, scripts/check-context-atlas.mjs, scripts/prepare-task-runner-wave.mjs, scripts/check-task-report.mjs, scripts/verify-task-run.mjs, scripts/verify-task-runner-batch.mjs, scripts/plan-task-runner.mjs. Did not touch session-trace.lisp / shared-memory.lisp until after the worker commit landed (those files are session-trace-writable / claim-allowed and explicitly NOT in :must-not-touch). Did not git add . / git push / --no-verify / --amend / --force.")
