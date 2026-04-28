;; Wave 28 / Task 02 — Task runner plan CLI v0.
;; Schema: missiond.report-contract.v1
;; Source: .missiond/tasks/wave28/wave28-02-task-runner-plan-cli-v0.lisp

(report wave28-02-task-runner-plan-cli-v0
  :schema "missiond.report-contract.v1"
  :task_id "wave28-02-task-runner-plan-cli-v0"
  :status done
  :commit_hash "302330a"
  :agent_commit_hash "954116e513c5"
  :final_commit_hash "302330a"
  :verified_commit_hash "302330a"
  :parent_patches
    [(:commit "302330a"
      :kind lint-cleanup
      :reason "Parent hotfix marked unused findCycle first parameter as _allNodes to clear TS6133 after the worker commit."
      :files ["scripts/plan-task-runner.mjs"])]
  :files_changed
    ["scripts/plan-task-runner.mjs"]

  :acceptance_results
    [(:command "node scripts/plan-task-runner.mjs --dry-fixture"
      :exit_code 0
      :ok true
      :notes "task-runner-plan fixtures OK (12 cases, 11 categories): pass-valid (1: 3-node A/B/C groups, batches/critical-path/tier-counts asserted), pass-determinism (1: same nodes reordered yield byte-identical output), pass-parallel (1: 3 nodes same group same depth -> single batch width=3), pass-critical-path (1: 10/20/30 chain -> 60), pass-overlap-error (1: same-group overlap with overlap_policy=warn -> severity=warning), pass-overlap-cross-group (1: cross-group overlap -> zero diagnostics), pass-tier-summary (1: 2 local + 1 smoke + 0 full counted), pass-overlap-reject-via-schema (1: default reject blocks at validateManifestObject before planner runs), fail-cycle (2: a<->b 2-node + a->b->c->a 3-node), fail-missing-task (1: on-disk join surfaces ghost task contract), fail-schema (1: bad verification_tier short-circuits via wave28-01 schema validator).")
     (:command "node scripts/check-task-runner-manifest.mjs --dry-fixture"
      :exit_code 0
      :ok true
      :notes "wave28-01 fixtures unchanged at 20 cases / 15 categories — no regression after wave28-02 imports projectManifest / readManifestFile / validateManifestObject / VERIFICATION_TIERS / OVERLAP_POLICIES / SCHEMA from check-task-runner-manifest.mjs.")
     (:command "node scripts/check-task-contract.mjs --all"
      :exit_code 0
      :ok true
      :notes "task-contract check OK (98 tasks) — wave22..wave28 task contracts including wave28-02 parse and pass shape / scope / must-not-touch / acceptance / commit-policy validation. No regressions.")
     (:command "git diff --check -- scripts/plan-task-runner.mjs"
      :exit_code 0
      :ok true
      :notes "no whitespace errors on the staged path; trailing-newline / tab-stop hygiene clean on the NEW file.")
     (:command "node scripts/check-missiond-hooks.mjs --json"
      :exit_code 0
      :ok true
      :notes "preflight OK; core.hooksPath aligned to .githooks; .githooks/pre-commit exists and is executable; no install required.")
     (:command "node scripts/task-scope-guard.mjs --task .missiond/tasks/wave28/wave28-02-task-runner-plan-cli-v0.lisp --mode staged"
      :exit_code 0
      :ok true
      :notes "task-scope-guard staged OK: wave28-02-task-runner-plan-cli-v0 (1 staged file) — staged path inside :write-scope; zero matches against :must-not-touch (crates/** .missiond/v2/** .missiond/router/** .missiond/tasks/wave27/** .missiond/tasks/wave28/wave28-*.lisp .missiond/tasks/wave28/dispatch-plan.lisp .missiond/claudecode/** scripts/{check-task-runner-manifest,render-claudecode-task,render-wave-briefs,verify-task-runner-batch}.mjs).")
     (:command "node scripts/verify-task-contract.mjs .missiond/tasks/wave28/wave28-02-task-runner-plan-cli-v0.lisp"
      :exit_code 0
      :ok true
      :notes "task-contract verify OK after parent hotfix: final commit 302330a preserves the same task message/scope and changes only scripts/plan-task-runner.mjs; original worker commit was 954116e513c5. Commit lineage is recorded in :agent_commit_hash / :final_commit_hash / :verified_commit_hash / :parent_patches.")]

  :scope_deviations []

  :trace_refs [wave28-02-trace-start-001 wave28-02-trace-commit-001 wave28-02-trace-complete-001]

  :major_decisions
    [(:decision "Reused wave28-01's projectManifest / readManifestFile / validateManifestObject as the single source of truth for manifest shape; the planner only adds graph + plan computation."
      :rationale "Brief explicitly forbids duplicating validation rules. validateManifestObject already mirrors the on-disk validator's enums / overlap rules / productive-only gate; wrapping it as a short-circuit at the top of planFromManifestObject keeps the planner focused on Kahn / critical-path / overlap-bucketing while the schema authority stays in one place.")
     (:decision "Topological batches respect both :depends_on edges AND :dispatch_group boundaries — nodes in different groups NEVER share a batch even when topologically equivalent."
      :rationale "Brief: 'each batch respects :dispatch_group boundaries — nodes in different groups never share a batch even if topologically equivalent.' Implemented by grouping the Kahn ready-set by dispatch_group and emitting one batch per group sorted lexicographically. This means a graph with [A1, A2 (both 0-deg)] in group A and [B1 (0-deg)] in group B yields two batches [[A1,A2], [B1]] rather than one mixed batch — the runner can dispatch each batch to a single group's worker pool without straddling.")
     (:decision "Cycle detection runs ONLY when Kahn's algorithm wedges (zero ready-set with non-empty remaining); the cycle walker emits a representative chain by following the first sorted in-cycle dependency from the lex-smallest remaining node."
      :rationale "wave28-01's :depends_on validator catches self-edges and missing references but not cross-edge cycles — that's the planner's responsibility per the brief's structured-cycle-error requirement. Returning {ok:false, error:'cycle', cycle:[...]} (with a deterministic chain) gives the caller exactly the structured error shape the brief asks for.")
     (:decision "Missing-task-contract on-disk join is a hard error (exit non-zero) and runs ONLY when checkTaskContractsOnDisk is true; fixtures default to false so they exercise the planner without needing wave99 contracts to exist."
      :rationale "The brief requires the CLI fail when a manifest references a non-existent task contract .lisp. Splitting the on-disk join into an option keeps the in-memory pipeline (planFromManifestObject) usable from fixtures and from future tooling that emits manifests transiently. The CLI entry point passes checkTaskContractsOnDisk:true so the on-disk guarantee is always enforced when reading from a real file.")
     (:decision "Determinism via two layers: (1) sorted by task_id at every batch construction site; (2) a recursive sortKeys() helper that re-emits nested objects with lexicographically-sorted keys before JSON.stringify."
      :rationale "Brief: 'topological sort MUST be deterministic (stable sort by node id within each batch)' and the determinism fixture asserts byte-identical output across two input orderings of the same nodes. sortKeys covers verification_tier_counts ordering (full < local < smoke alphabetically); per-array order is enforced explicitly at each construction site so we never accidentally rely on object iteration order.")
     (:decision "Verification-tier counts are pre-seeded to {local:0, smoke:0, full:0} so the output shape is invariant under input — a manifest with zero smoke nodes still emits 'smoke: 0' rather than dropping the key."
      :rationale "Predictable shape matters for the wave28-04 daemon dry-run surface and the wave28-05 batch verifier — they should not have to special-case missing tier keys. Mirrors the wave27-04 router-dispatch-descriptor pattern of pre-seeding all enum keys.")
     (:decision "Overlap diagnostics are aggregated per pair-of-task_ids (one row per pair, not one per overlapping path); paths inside the row are sorted and de-duplicated."
      :rationale "When two nodes claim three shared paths, surfacing three separate diagnostics drowns the signal. Aggregating by pair gives the caller one actionable row per coordination conflict; the paths array preserves the full evidence inside the row.")
     (:decision "Lisp output format renders ids as bare atoms when they match the wave28-01 id regex, otherwise quoted strings."
      :rationale "Round-trippable through the existing missiond_lisp parser without surprising readers — task ids like 'wave99-01-foo' stay unquoted (matching the same atom shape the manifest itself uses) while paths and free-form text always get quoted for safety.")
     (:decision "main() gated on `import.meta.url === \\`file://${process.argv[1]}\\`` so wave28-04 / 05 / 06 can import named exports without triggering CLI side-effects (mirrors check-task-runner-manifest.mjs and check-task-contract.mjs)."
      :rationale "Standard MissionD pattern. Importers like the wave28-04 daemon dry-run handler should not need to monkey-patch process.argv or worry about top-level CLI parsing firing.")]

  :time_sinks
    [(:label "Reading wave28-01 checker source + named exports + 20-fixture catalogue"
      :notes "Largest sink — needed to confirm exactly which exports are stable (SCHEMA / VERIFICATION_TIERS / OVERLAP_POLICIES / projectManifest / readManifestFile / validateManifestObject), how validateManifestObject handles cross-node overlap (so the planner's overlap pass mirrors severity correctly via overlap_policy=warn), and that wave28-01's :depends_on validator does NOT detect cross-edge cycles (so the planner owns that check).")
     (:label "Designing the dispatch_group-aware topological batching"
      :notes "Vanilla Kahn yields one batch per topological depth. The brief requires batches respect dispatch_group too — nodes in different groups never share a batch even when topologically equivalent. Final approach: at every Kahn iteration, partition the ready-set by dispatch_group and emit one batch per group (sorted lexicographically). This keeps the algorithm O(V+E) while honoring the group-boundary invariant.")
     (:label "Drafting 12 dry-fixture cases across 11 categories"
      :notes "Targeted 8-12, landed at 12. Critical coverage: pass-determinism (re-runs the planner on a permuted version of the canonical fixture and asserts JSON byte-equality after normalizing manifest_path), pass-overlap-reject-via-schema (proves default policy short-circuits at validateManifestObject before the planner runs its own overlap pass), fail-cycle x2 (2-node and 3-node), fail-missing-task (on-disk join), fail-schema (bad enum). One pass case per output field family: critical-path, parallel width, tier counts.")]

  :unexpected_work
    [(:summary "Ran end-to-end smoke against /tmp/wave28-manifest-smoke.lisp (a manifest naming the two real wave28 task contracts that exist on disk) to confirm both --json and --lisp output paths produce the expected schema fields with correct ordering. Useful as a regression spot-check; not stored in the repo since it duplicates the dispatch-plan content.")
     (:summary "Confirmed that when the on-disk join fails, the missing_task error fires BEFORE the cycle check would — meaning a manifest with both a missing-task and a cycle reports missing_task first. This is intentional: a missing task contract is unrecoverable, while a cycle is recoverable by editing the manifest, so the planner surfaces the harder failure first.")
     (:summary "Added recursive sortKeys() helper instead of relying on Object.keys insertion order. JSON.stringify in Node respects insertion order by default but assemblies inside the planner mutate intermediate objects; sortKeys gives a single normalization choke-point right before serialization.")]

  :recommended_backend "claudecode"
  :router_confidence "high"
  :router_policy_path ".missiond/router/router-policy-v1.lisp"
  :router_dry_run_only true
  :router_applied false
  :router_reasons
    ["Dispatch strategy fresh-code-alignment + owner claudecode → matches r-fresh-code-alignment-to-claudecode in router-policy-v1 (priority 100, single matched rule)."
     "Workstation surface (NEW Node.js CLI consuming wave28-01 schema validator; no Rust / SQL / cargo) is the canonical claudecode beat — pure file reads, no network / LLM call required."
     "Router output is recorded for telemetry only; runtime dispatch unchanged (claudecode is the live default and remained the live default for this task)."]
  :router_trace_index_path ".missiond/router/trace-index-v1.lisp"

  :router_backend_readiness_status "current-default"
  :router_backend_runtime_allowed true
  :router_apply_eligible false
  :router_apply_blockers
    ["current-default is the live runtime today but explicit runtime-ready opt-in is required upstream before this task's dispatch handoff would mark a descriptor as apply-eligible (wave27-01 eligibility-gate intentionally REJECTS current-default → eligible)."]
  :router_backend_registry_path ".missiond/router/router-backend-registry-v1.lisp"

  :notes
    "wave28-02 ships:
     - scripts/plan-task-runner.mjs (read-only plan CLI; never shells out, never touches git / network / LLM).

     CLI shape: node scripts/plan-task-runner.mjs --manifest <manifest.lisp> [--json|--lisp] [--dry-fixture].
       --manifest <path>  : load and plan a manifest from disk.
       --json (default)   : emit structured plan JSON (sorted keys, deterministic).
       --lisp             : emit S-expression plan (alt format for piping to a future verifier).
       --dry-fixture      : run 12 in-process pass/fail fixtures.

     Output JSON top-level keys (sorted lexicographically):
       schema                   = literal 'missiond.task-runner-plan.v0'
       manifest_path            = repo-relative when inside repo, absolute otherwise
       wave                     = mirrors manifest :wave
       productive_only          = mirrors manifest :productive_only literal bool
       batches                  = array of arrays of node ids; topological grouping by depth, partitioned by dispatch_group
       critical_path_minutes    = longest dependency chain by estimated_minutes
       total_estimated_minutes  = sum of all node estimated_minutes
       max_parallel_width       = max batch size across all batches
       overlap_diagnostics      = array of {pair:[a,b], paths:[...], severity, group} for same-group write_scope overlap
       verification_tier_counts = {local:int, smoke:int, full:int} — pre-seeded so the shape is invariant

     Cross-wave invariants enforced:
       1. Topological sort is deterministic — sorted by task_id within each batch + sortKeys() recursive normalization before JSON.stringify.
       2. Cycle detection: Kahn's algorithm wedge -> {ok:false, error:'cycle', cycle:[a, b, c, a]}. Lex-smallest remaining node walks the first sorted in-cycle dependency until it loops back.
       3. Missing task contract on disk: structured error {ok:false, error:'missing_task', missing:[{task_id, expected_path}]}. Only enforced when reading from a real manifest file (CLI path); fixtures opt out via checkTaskContractsOnDisk:false so they don't need wave99 contracts to exist.
       4. Schema validation short-circuits on validateManifestObject errors — wave28-01 stays the single source of truth for shape rules.
       5. Same-dispatch_group write_scope overlap surfaces in overlap_diagnostics with severity='error' (overlap_policy=reject) or severity='warning' (overlap_policy=warn). Cross-group overlap produces ZERO diagnostics (allowed by wave28-01 default policy).
       6. Batches respect dispatch_group boundaries — nodes from different groups NEVER share a batch even when topologically equivalent.

     Hard guarantees verified by `grep -nE 'child_process|spawn|fetch|http|https|exec|git|openai|anthropic' scripts/plan-task-runner.mjs` returning ZERO matches:
       - No dispatch / no mission_task_delegate.
       - No child_process.spawn / exec / fork.
       - No git.
       - No HTTP / network / fetch.
       - No LLM (openai / anthropic / etc).
       Pure planning over file reads via fs.readFileSync (the missiond_lisp reader) + fs.existsSync (on-disk join only).

     Named exports for wave28-04 (daemon dry-run surface) / wave28-05 (batch verifier) / wave28-06 (loop smoke):
       PLAN_SCHEMA              = literal 'missiond.task-runner-plan.v0'
       MANIFEST_SCHEMA          = re-export from wave28-01
       VERIFICATION_TIERS       = re-export from wave28-01
       OVERLAP_POLICIES         = re-export from wave28-01
       planFromManifestFile(path, opts?)   = on-disk planning entry; runs schema validation + on-disk join + plan computation
       planFromManifestObject(manifest, opts?)  = in-memory planning entry; schema validation + plan computation; opt-in on-disk join
       readLispFile (re-exported from lib/missiond_lisp)

     main() gated on `import.meta.url === \`file://${process.argv[1]}\`` so importers do not accidentally trigger CLI side-effects.

     Dry-fixture totals: 12 cases across 11 categories — pass-valid (1: 3-node A/B/C groups), pass-determinism (1: byte-identical output across input permutation), pass-parallel (1: 3 nodes same group same depth -> 1 batch width=3), pass-critical-path (1: 10/20/30 chain -> 60), pass-overlap-error (1: warn severity), pass-overlap-cross-group (1: zero diagnostics), pass-tier-summary (1), pass-overlap-reject-via-schema (1: default reject blocks at schema), fail-cycle (2: 2-node + 3-node), fail-missing-task (1), fail-schema (1).

     Pre-commit pipeline:
       check-task-runner-manifest.mjs --dry-fixture (exit=0, 20/20)
       -> plan-task-runner.mjs --dry-fixture (exit=0, 12/12)
       -> check-task-contract.mjs --all (exit=0, 98 tasks)
       -> git diff --check (exit=0)
       -> check-missiond-hooks.mjs --json (preflight aligned)
       -> git add scripts/plan-task-runner.mjs (only path staged)
       -> task-scope-guard.mjs --mode staged (OK, 1 staged)
       -> MISSIOND_TASK_CONTRACT=... git commit -m 'feat(tasks): add task runner plan CLI' (worker commit 954116e513c5)
       -> parent hotfix commit 302330a (TS6133 unused parameter cleanup, same write-scope)
       -> verify-task-contract.mjs rerun against final commit 302330a.

     Append-only ledger updates: shared-memory wave28-02-claim-001 (seq 4) before staging + wave28-02-completion-001 (seq 6) after verify; session-trace wave28-02-trace-start-001 (seq 5) before reading background + wave28-02-trace-commit-001 (seq 7, with commit hash) + wave28-02-trace-complete-001 (seq 8). Both ledgers re-validated after each append. Fresh tail-read before each append because the parallel wave28-03 agent appended its own claim/start (seq 5 in shared-memory, seq 6 in session-trace) between this task's claim and completion.

     Constraints honored: NO Rust / SQL / Cargo edits. Did not touch crates/**, .missiond/v2/**, .missiond/router/**, schemas (task-contract / task-runner-manifest), .missiond/tasks/wave27/**, any wave28-*.lisp other than session-trace + shared-memory (both are session-trace-writable / claim-allowed and explicitly NOT in :must-not-touch), .missiond/tasks/wave28/dispatch-plan.lisp, .missiond/claudecode/**, scripts/check-task-runner-manifest.mjs, scripts/render-claudecode-task.mjs, scripts/render-wave-briefs.mjs, scripts/verify-task-runner-batch.mjs. Did not git add . / git push / --no-verify / --amend / --force.")
