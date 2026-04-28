;; Wave 28 / Task 03 — Wave brief batch renderer v0.
;; Schema: missiond.report-contract.v1
;; Source: .missiond/tasks/wave28/wave28-03-wave-brief-batch-renderer-v0.lisp

(report wave28-03-wave-brief-batch-renderer-v0
  :schema "missiond.report-contract.v1"
  :task_id "wave28-03-wave-brief-batch-renderer-v0"
  :status done
  :commit_hash "c4a985036040"
  :files_changed
    ["scripts/render-wave-briefs.mjs"
     "scripts/render-claudecode-task.mjs"]

  :acceptance_results
    [(:command "node scripts/render-wave-briefs.mjs --dry-fixture"
      :exit_code 0
      :ok true
      :notes "render-wave-briefs fixtures OK (9 cases, 9 categories): pass-render-from-scratch, pass-thin-brief-shape, pass-deterministic-paths, pass-skip-when-present, pass-force-overwrites, fail-archive-id, fail-backfill-id, fail-index-kind, pass-backward-compat (in-process smoke that exported renderTask thin + full + renderSharedPreamble + deriveSharedPreamblePath behave correctly).")
     (:command "node scripts/render-claudecode-task.mjs --dry-fixture"
      :exit_code 0
      :ok true
      :notes "render-claudecode-task fixtures OK (6 cases, 6 categories) — byte-identical regression check after wave28-03 added named exports and gated main() on direct invocation. Categories: wave26-06-renderer-readiness-literals, wave26-06-renderer-static-audit, wave27-05-renderer-dispatch-descriptor-literals, wave27-05-renderer-static-audit, wave27-06-renderer-literals, wave28-dispatch-efficiency.")
     (:command "node scripts/check-task-contract.mjs --all"
      :exit_code 0
      :ok true
      :notes "task-contract check OK (98 tasks) — every wave22..wave28 contract still parses + passes shape / scope / must-not-touch / acceptance / commit-policy validation. No regressions vs the wave28-02 baseline at commit 954116e.")
     (:command "git diff --check -- scripts/render-wave-briefs.mjs scripts/render-claudecode-task.mjs"
      :exit_code 0
      :ok true
      :notes "no whitespace errors on either staged path; trailing-newline / tab-stop hygiene clean on the new file (render-wave-briefs.mjs) and the surgical edit (render-claudecode-task.mjs).")
     (:command "node scripts/check-missiond-hooks.mjs --json"
      :exit_code 0
      :ok true
      :notes "preflight OK; core.hooksPath aligned to .githooks; .githooks/pre-commit exists and is executable; no install required.")
     (:command "node scripts/task-scope-guard.mjs --task .missiond/tasks/wave28/wave28-03-wave-brief-batch-renderer-v0.lisp --mode staged"
      :exit_code 0
      :ok true
      :notes "task-scope-guard staged OK: wave28-03-wave-brief-batch-renderer-v0 (2 staged file(s)) — both staged paths inside :write-scope; zero matches against :must-not-touch (crates/** .missiond/v2/** .missiond/router/** .missiond/tasks/wave27/** .missiond/tasks/wave28/wave28-*.lisp .missiond/tasks/wave28/dispatch-plan.lisp .missiond/claudecode/wave27-*.md scripts/{check-task-runner-manifest,plan-task-runner,verify-task-runner-batch}.mjs).")
     (:command "MISSIOND_TASK_CONTRACT=.missiond/tasks/wave28/wave28-03-wave-brief-batch-renderer-v0.lisp git commit -m \"feat(tasks): render wave briefs from manifest\""
      :exit_code 0
      :ok true
      :notes "commit c4a9850036040 created. Pre-commit hook (.githooks/pre-commit) re-ran the staged scope guard and accepted the 2 staged files. Commit message matches task-contract :commit :message verbatim.")
     (:command "node scripts/verify-task-contract.mjs .missiond/tasks/wave28/wave28-03-wave-brief-batch-renderer-v0.lisp"
      :exit_code 0
      :ok true
      :notes "task-contract verify OK: wave28-03-wave-brief-batch-renderer-v0 against c4a985036040 — commit hash exists; commit message matches `feat(tasks): render wave briefs from manifest` per contract; changed_files ⊆ write-scope (write-scope-only); changed_files ∩ must-not-touch = ∅; acceptance commands present in contract.")]

  :scope_deviations []

  :trace_refs [wave28-03-trace-start-001 wave28-03-trace-commit-001 wave28-03-trace-complete-001]

  :major_decisions
    [(:decision "Add named exports (loadSingleTask / renderTask / renderSharedPreamble / deriveSharedPreamblePath) to scripts/render-claudecode-task.mjs and gate main() on import.meta.url === `file://${process.argv[1]}`. The new batch renderer imports those functions in-process — never shells out."
      :rationale "The contract explicitly forbids shelling out to render-claudecode-task.mjs. Surgical Edit anchors only — every existing dry-fixture stays byte-identical at runtime (render-claudecode-task --dry-fixture re-ran 6/6 OK). The same gating pattern (import.meta.url) is already used by scripts/check-task-runner-manifest.mjs from wave28-01, so the convention is wave-consistent.")
     (:decision "Defence-in-depth rejection of archive / backfill / index / lisp-backfill nodes inside renderManifest, on top of the wave28-01 checker rejection."
      :rationale "wave28-01's check-task-runner-manifest.mjs rejects these node shapes when :productive_only true. The brief still calls for the renderer to fail loudly if such a node ever lands — fixtures synthesize manifests with :productive_only false so wave28-01 accepts them, then prove our renderer still rejects each shape. Belt-and-braces: the orchestrator-owned pseudo-nodes (archive / backfill / index / lisp-backfill) MUST NEVER be rendered as worker briefs.")
     (:decision "Skip-when-present for the shared preamble, with --force as the explicit overwrite opt-in. Each thin brief obeys the same rule."
      :rationale "The user already shipped .missiond/claudecode/wave28-shared-preamble.md and the per-task wave28-*.md briefs in commit 9371fc5. The brief explicitly forbids overwriting these in this commit. skip-when-present preserves the existing bytes; --force is the explicit user-driven opt-in for re-render. Both behaviors are exercised by dedicated dry-fixtures inside a tmp dir so the live repo files are never touched.")
     (:decision "Reuse renderSharedPreamble named export (single source of truth for preamble text) instead of duplicating the preamble template in render-wave-briefs.mjs."
      :rationale "Two copies of the same boilerplate text would drift. The render-claudecode-task.mjs --brief-mode preamble path and render-wave-briefs.mjs both call the same renderSharedPreamble() so the bytes are guaranteed identical. The wave28-03 dry-fixture explicitly asserts the preamble carries the canonical sections (Shared Memory / Report Contract / Commit Protocol).")
     (:decision "Async runFixtures with a process.exit(2) -> throw shim during the fixture loop so per-fixture try/catch can assert on rejection messages without tearing down the whole suite."
      :rationale "The production code path uses fail() -> process.exit(2) for fast-fail behavior. Fixtures need to assert the error message text (e.g. that '-archive-' substring detection fired). The shim is scoped to runFixtures only via patch/unpatch so the production path is untouched; mainAsync only patches when --dry-fixture is the entry point.")
     (:decision "Defer loadSingleTask file-not-found errors to renderManifest, but pre-check the manifest path itself in renderManifest. The renderer fails fast with a clear path before trying to load any task contract."
      :rationale "Two-stage error surface keeps the message attributable. A missing manifest is a CLI / orchestration error (caller mistake); a missing task contract referenced inside the manifest is a manifest-vs-repo mismatch. Both are fail-fast (no fallback / silent skip), but the messages tell the operator which side to fix.")]

  :time_sinks
    [(:label "Reading existing renderer source + verifying which functions to export"
      :notes "render-claudecode-task.mjs is 1070 lines and carries multiple cross-wave invariants (wave24-05 / wave25-04 / wave26-05 / wave27-05). Reading the full file end-to-end was necessary to pick the smallest-surface set of named exports (4 functions) that lets render-wave-briefs.mjs run in-process without touching the renderTask body or its CLI argument parser.")
     (:label "Designing the dry-fixture suite for the rejection paths"
      :notes "The defence-in-depth fail fixtures need to smuggle archive / backfill / index node shapes past wave28-01's productive_only checker (otherwise the checker rejects the manifest first, never reaching our renderer). Solution: synthesize manifests with :productive_only false and a single forbidden node, which wave28-01 accepts; render-wave-briefs.mjs is then the second guard that rejects the node shape. The async process.exit shim was the bridge that let the fixtures assert the error message text without tearing down the suite.")
     (:label "Confirming wave28-02's parallel work did not touch our write_scope"
      :notes "wave28-02 (parallel agent in dispatch_group B) was active on scripts/plan-task-runner.mjs. Its task contract lists scripts/render-claudecode-task.mjs and scripts/render-wave-briefs.mjs in must-not-touch, and our task contract lists scripts/plan-task-runner.mjs in must-not-touch. Final git status before stage confirmed zero overlap; wave28-02 committed at 954116e while wave28-03 was implementing.")]

  :unexpected_work
    [(:summary "Discovered render-claudecode-task.mjs unconditionally invoked main() at the bottom of the file, which would have triggered the CLI when imported. Adding the import.meta.url gate is the standard wave28-01 pattern (check-task-runner-manifest.mjs) and is the only way to safely import the renderer functions. Gating is byte-identical for direct invocation: existing dry-fixtures re-ran 6/6 OK after the gate.")
     (:summary "Built an in-process backward-compatibility smoke fixture inside render-wave-briefs.mjs that re-asserts loadSingleTask + renderTask (both thin and full modes) + renderSharedPreamble + deriveSharedPreamblePath all behave correctly post-export. Independent of the on-disk dry-fixture suite — defence in depth against future renderer churn that might silently break the named-export surface.")]

  :recommended_backend "claudecode"
  :router_confidence "high"
  :router_policy_path ".missiond/router/router-policy-v1.lisp"
  :router_dry_run_only true
  :router_applied false
  :router_reasons
    ["Dispatch strategy fresh-code-alignment + owner claudecode → matches r-fresh-code-alignment-to-claudecode in router-policy-v1 (priority 100, single matched rule)."
     "Workstation surface (NEW Node.js batch renderer + surgical Edit on existing renderer for named exports + main() gating; no Rust / SQL / cargo) is the canonical claudecode beat — no network / LLM call required from the worker side."
     "Router output is recorded for telemetry only; runtime dispatch unchanged (claudecode is the live default and remained the live default for this task)."]

  :router_backend_readiness_status "current-default"
  :router_backend_runtime_allowed true
  :router_apply_eligible false
  :router_apply_blockers
    ["current-default is the live runtime today but explicit runtime-ready opt-in is required upstream before this task's dispatch handoff would mark a descriptor as apply-eligible (wave27-01 eligibility-gate intentionally REJECTS current-default → eligible)."]
  :router_backend_registry_path ".missiond/router/router-backend-registry-v1.lisp"

  :notes
    "wave28-03 ships:
     - scripts/render-wave-briefs.mjs (NEW batch renderer; 9 dry-fixtures across 9 categories all PASS; never shells out — imports renderer functions in-process).
     - scripts/render-claudecode-task.mjs (Edit only; +5 named exports `loadSingleTask` / `renderTask` / `renderSharedPreamble` / `deriveSharedPreamblePath` (loadSingleTask was internal; the others were already module-internal helpers) + main() gated on import.meta.url. Existing 6 dry-fixtures stay green BYTE-IDENTICAL.)

     CLI:
       node scripts/render-wave-briefs.mjs --manifest <manifest.lisp> [--force] [--dry-fixture]

     Behavior:
       1. Read + validate the manifest via wave28-01 readManifestFile + validateManifestObject (fail fast on schema errors).
       2. Defence in depth: reject any node whose :kind ∈ {archive | backfill | index | lisp-backfill} or whose :task_id contains '-archive-' / '-backfill-' / '-index' / 'lisp-backfill' substring (even if the manifest is :productive_only false and wave28-01 let it through).
       3. Resolve task contracts at .missiond/tasks/<wave>/<task-id>.lisp (fail with a clear path when missing).
       4. Render the shared preamble ONCE at <manifest>:shared_preamble_path:
          - skipped (action=skipped) when present and --force is not set;
          - written (action=written) when missing;
          - overwritten (action=overwritten) when present and --force is set.
       5. Render each productive node as a thin brief at .missiond/claudecode/<task-id>.md with --brief-mode thin and --shared-preamble pointing at the manifest-declared preamble path. Same skip/force semantics per brief.

     Output convention (deterministic):
       preamble path:  .missiond/claudecode/<wave>-shared-preamble.md  (typical; whatever <manifest>:shared_preamble_path declares)
       thin brief path: .missiond/claudecode/<task-id>.md  (one per productive node)

     render-claudecode-task.mjs Edit summary (surgical):
       1. `function loadSingleTask` -> `export function loadSingleTask`
       2. `function renderTask`     -> `export function renderTask`
       3. `function renderSharedPreamble` -> `export function renderSharedPreamble`
       4. `function deriveSharedPreamblePath` -> `export function deriveSharedPreamblePath`
       5. `main();` (top-level) -> `if (import.meta.url === \\`file://${process.argv[1]}\\`) { main(); }` with explanatory comment.
     No other lines touched. Existing renderer dry-fixtures (wave26-06 / wave27-05 / wave27-06 / wave28 thin-brief — 6 fixtures across 6 categories) continue to pass byte-identical (verified pre-stage and post-stage).

     Dry-fixture totals (render-wave-briefs.mjs): 9 cases across 9 categories.
       pass-render-from-scratch (1): preamble + 2 thin briefs written into a fresh tmp repo skeleton from a 2-node manifest.
       pass-thin-brief-shape (1): thin brief MUST point at the manifest preamble path AND MUST NOT contain any of '## Shared Memory' / '## Report Contract' / '## Session Trace' / '## Router Policy' (those sections live in the preamble in thin mode).
       pass-deterministic-paths (1): two consecutive --force renders produce byte-identical outputs at the same paths.
       pass-skip-when-present (1): pre-seeded sentinel preamble bytes are preserved; renderer reports action=skipped.
       pass-force-overwrites (1): pre-seeded stale preamble is replaced with the canonical bytes; renderer reports action=overwritten.
       fail-archive-id (1): node task_id 'wave99-00-archive-foo' rejected; error message contains '-archive-'.
       fail-backfill-id (1): node task_id 'wave99-09-lisp-backfill-status' rejected; error message contains 'lisp-backfill'/'-backfill-'.
       fail-index-kind (1): node task_id has no forbidden substring but :kind is 'index'; error message contains 'index'.
       pass-backward-compat (1): in-process smoke that exported renderTask thin + full + renderSharedPreamble + deriveSharedPreamblePath all behave correctly post-export (asserts thin brief shape, full brief richer surface, preamble canonical sections, and deriveSharedPreamblePath returns wave-prefixed path).

     Cross-wave invariants honored:
       - Manifests stay advisory orchestration metadata, NEVER a runtime backend switch.
       - Renderer never shells out (no child_process / spawn / fork / exec / fetch / http(s).get|request|post / simpleGit). Verified by reading the file end-to-end and by the wave26-06 / wave27-05 static-audit fixtures still passing on render-claudecode-task.mjs after our edit.
       - The user's checked-in .missiond/claudecode/wave28-*.md briefs and wave28-shared-preamble.md are NEVER touched by this commit (write_scope is scripts/ only); fixtures use tmp dirs.

     Pre-commit pipeline: render-wave-briefs --dry-fixture (exit=0, 9/9 OK) → render-claudecode-task --dry-fixture (exit=0, 6/6 OK byte-identical) → check-task-contract --all (exit=0, 98 tasks) → git diff --check (exit=0) → check-missiond-hooks --json (preflight aligned) → git add (2 paths) → task-scope-guard --mode staged (OK, 2 staged) → MISSIOND_TASK_CONTRACT=... git commit (commit c4a985036040) → verify-task-contract (OK against c4a985036040). Append-only ledger updates: shared-memory wave28-03-claim-001 (seq 5) before staging + wave28-03-completion-001 (seq 7) after verify; session-trace wave28-03-trace-start-001 (seq 6) before reading background + wave28-03-trace-commit-001 (seq 9, with commit hash) + wave28-03-trace-complete-001 (seq 10). Both ledgers re-validated after each append.

     Constraints honored: NO Rust / SQL / Cargo edits. Did not touch crates/**, .missiond/v2/**, .missiond/router/**, .missiond/tasks/schema/**, .missiond/tasks/wave27/**, any wave28-*.lisp other than session-trace + shared-memory (both are session-trace-writable / claim-allowed and explicitly NOT in :must-not-touch), .missiond/tasks/wave28/dispatch-plan.lisp, .missiond/claudecode/wave27-*.md, scripts/check-task-runner-manifest.mjs, scripts/plan-task-runner.mjs, scripts/verify-task-runner-batch.mjs. Did not git add . / git push / --no-verify / --amend / --force.")
