;; Wave 28 / Task 01 — Task runner manifest schema v0.
;; Schema: missiond.report-contract.v1
;; Source: .missiond/tasks/wave28/wave28-01-task-runner-manifest-schema-v0.lisp

(report wave28-01-task-runner-manifest-schema-v0
  :schema "missiond.report-contract.v1"
  :task_id "wave28-01-task-runner-manifest-schema-v0"
  :status done
  :commit_hash "145645c2fd5a"
  :files_changed
    [".missiond/tasks/schema/task-runner-manifest-v1.lisp"
     "scripts/check-task-runner-manifest.mjs"]

  :acceptance_results
    [(:command "node scripts/check-task-runner-manifest.mjs --dry-fixture"
      :exit_code 0
      :ok true
      :notes "task-runner-manifest fixtures OK (20 cases, 15 categories): pass / pass-overlap-warn / pass-overlap-cross-group / fail-duplicate / fail-missing-dep / fail-self-edge / fail-enum (verification_tier + brief_mode) / fail-overlap / fail-positive-int (zero heartbeat + negative estimated) / fail-string-bool / fail-productive-only (archive id + backfill id + index :kind) / fail-path (absolute write_scope + traversal preamble) / fail-unknown-entry-head / fail-unknown-field / fail-missing-required. JSON variant verified separately (--json --dry-fixture) and reports the same totals.")
     (:command "node scripts/check-task-contract.mjs --all"
      :exit_code 0
      :ok true
      :notes "task-contract check OK (98 tasks) — all wave22..wave28 task contracts including this one parse and pass shape / scope / must-not-touch / acceptance / commit-policy validation. No regressions vs the wave27 baseline.")
     (:command "git diff --check -- .missiond/tasks/schema/task-runner-manifest-v1.lisp scripts/check-task-runner-manifest.mjs"
      :exit_code 0
      :ok true
      :notes "no whitespace errors on either staged path; trailing-newline / tab-stop hygiene clean on both NEW files.")
     (:command "node scripts/check-missiond-hooks.mjs --json"
      :exit_code 0
      :ok true
      :notes "preflight OK; core.hooksPath aligned to .githooks; .githooks/pre-commit exists and is executable; no install required.")
     (:command "node scripts/task-scope-guard.mjs --task .missiond/tasks/wave28/wave28-01-task-runner-manifest-schema-v0.lisp --mode staged"
      :exit_code 0
      :ok true
      :notes "task-scope-guard staged OK: wave28-01-task-runner-manifest-schema-v0 (2 staged file(s)) — both staged paths inside :write-scope; zero matches against :must-not-touch (crates/** .missiond/v2/** .missiond/router/** .missiond/tasks/wave27/** .missiond/tasks/wave28/wave28-*.lisp .missiond/tasks/wave28/dispatch-plan.lisp .missiond/claudecode/** scripts/{render-claudecode-task,check-task-contract,plan-task-runner,render-wave-briefs,verify-task-runner-batch}.mjs).")
     (:command "node scripts/verify-task-contract.mjs .missiond/tasks/wave28/wave28-01-task-runner-manifest-schema-v0.lisp"
      :exit_code 0
      :ok true
      :notes "task-contract verify OK: wave28-01-task-runner-manifest-schema-v0 against 145645c2fd5a — commit hash exists; commit message matches `feat(tasks): add task runner manifest schema` per contract; changed_files ⊆ write-scope (write-scope-only); changed_files ∩ must-not-touch = ∅; acceptance commands present in contract.")]

  :scope_deviations []

  :trace_refs [wave28-01-trace-start-001 wave28-01-trace-commit-001 wave28-01-trace-complete-001]

  :major_decisions
    [(:decision "Top-level form named (task-runner-manifest ...) per the brief — MANIFEST shape, distinct from the existing dispatch-plan.lisp form."
      :rationale "The wave28 dispatch-plan.lisp is hand-authored orchestrator policy and stays orchestrator-owned. The manifest schema describes the same node graph in a machine-emittable form so wave28-02 plan CLI / wave28-03 brief renderer / wave28-05 batch verifier have a structured source. Using a distinct head atom keeps the two surfaces unambiguous and avoids accidentally reusing dispatch-plan as a runtime backend switch.")
     (:decision "verification_tier enum mirrors task-contract v1 exactly: {local | smoke | full}. No new tiers."
      :rationale "Drift between the two enums is a structural error per the schema's cross-wave invariant. The brief mandates this; the checker enforces it. wave28-02..06 should never have to guess which tier set is canonical.")
     (:decision "Default :overlap_policy = reject. The :overlap_policy warn opt-in is reserved for explicitly serialized retry/repair manifests."
      :rationale "Two parallel agents writing the same file inside the same dispatch_group is a coordination bug — that's the entire reason wave28 promotes manifests to a structured surface. Rejecting by default keeps productive_only manifests safe; warn exists so a future retry/repair manifest can declare intent without re-implementing the rule.")
     (:decision "productive_only=true gate inspects BOTH :kind (canonical) AND :task_id substrings (defence in depth)."
      :rationale "Today's wave dispatch-plan policy says archive/backfill/index/lisp-backfill orchestrator tasks MUST NOT appear as worker nodes. :kind is the canonical signal but legacy task ids carrying `-archive-` / `-backfill-` / `-index` / `lisp-backfill` predate the convention. Substring match catches them even when a node forgets to declare :kind.")
     (:decision "estimated_minutes / heartbeat_minutes are positive integers (^[1-9][0-9]*$), literal atoms, NEVER strings."
      :rationale "Mirrors the wave27-01 literal-bool pattern. Zero / negative / fractional / string forms are all structural errors. Forces the emitter to commit to a real number; no `\"unknown\"` escape hatch.")
     (:decision "shared_preamble_path missing is an error when :brief_mode is thin|preamble; missing on disk is a WARNING only."
      :rationale "Schema does not own the preamble file; the wave28-05 batch verifier owns the hard cross-file join. Warning lets ad hoc fixture files validate cleanly while still surfacing real drift in CI logs. The task contract path validation prevents absolute / ~ / .. paths regardless.")
     (:decision "Named exports include validateManifestObject (object-shape mirror of the on-disk validator)."
      :rationale "wave28-02 will emit manifests as in-memory objects from the plan CLI before serializing. wave28-05 will load manifests via readManifestFile and run cross-file joins. A single import that runs the same enum / overlap / productive-only rules eliminates duplication and keeps the checker as the single source of truth for manifest shape.")]

  :time_sinks
    [(:label "Reading background schemas + checkers (router-dispatch-descriptor, router-backend-registry, report-contract, task-contract)"
      :notes "Largest sink — 4 files were load-bearing for the design. router-dispatch-descriptor was the closest analog (named exports / locked invariants / gated main / --stdin / --dry-fixture / projectX + readXFile + validateXObject pattern). task-contract supplied the verification-tier enum + the kind set.")
     (:label "Designing the same-dispatch-group write-scope overlap rule"
      :notes "Per-node validation is local; same-group overlap is a cross-node graph check. Bucketed by dispatch_group, then by path; flag pairs from DIFFERENT task_ids (a node legitimately listing the same path twice in its own write_scope is a separate `duplicate path inside same node` error). Severity gated by :overlap_policy (default reject; warn lowers to advisory).")
     (:label "Drafting 20 dry-fixture cases across 15 categories"
      :notes "Targeted ≥12, landed at 20 to cover each rejection rule plus 3 happy-path variants (valid productive 2-group, overlap_policy=warn allowing same-file overlap, cross-group overlap allowed). Productive-only gate has 3 distinct fixtures (archive id / backfill id / index :kind) to prove both substring + :kind detection paths fire independently.")]

  :unexpected_work
    [(:summary "Added FORBIDDEN_PRODUCTIVE_KINDS as an explicit named export (instead of inlining the strings) so wave28-02 / wave28-05 can re-use the same enum when generating / verifying manifests. Same export pattern as wave27-01 BACKEND_IDS / READINESS_STATUSES.")
     (:summary "Added a duplicate-path-inside-same-node check distinct from the cross-node dispatch_group overlap check. Catches authoring typos (`[\"a.mjs\" \"a.mjs\"]`) per node before they would silently confuse the cross-node bucket counter.")
     (:summary "Added warnings vector to the JSON shape ({ ok, files, errors[], warnings[], manifests_validated, nodes_validated }) since :overlap_policy=warn and missing-shared-preamble produce non-fatal advisory output. wave27-01's checker had no warnings concept; mirrors the daemon's diagnostics convention.")]

  :recommended_backend "claudecode"
  :router_confidence "high"
  :router_policy_path ".missiond/router/router-policy-v1.lisp"
  :router_dry_run_only true
  :router_applied false
  :router_reasons
    ["Dispatch strategy fresh-code-alignment + owner claudecode → matches r-fresh-code-alignment-to-claudecode in router-policy-v1 (priority 100, single matched rule)."
     "Workstation surface (NEW Lisp schema + new Node.js checker, no Rust / SQL / cargo) is the canonical claudecode beat — no network / LLM call required from the worker side."
     "Router output is recorded for telemetry only; runtime dispatch unchanged (claudecode is the live default and remained the live default for this task)."]
  :router_trace_index_path ".missiond/router/trace-index-v1.lisp"

  :router_backend_readiness_status "current-default"
  :router_backend_runtime_allowed true
  :router_apply_eligible false
  :router_apply_blockers
    ["current-default is the live runtime today but explicit runtime-ready opt-in is required upstream before this task's dispatch handoff would mark a descriptor as apply-eligible (wave27-01 eligibility-gate intentionally REJECTS current-default → eligible)."]
  :router_backend_registry_path ".missiond/router/router-backend-registry-v1.lisp"

  :notes
    "wave28-01 ships:
     - .missiond/tasks/schema/task-runner-manifest-v1.lisp (schema id missiond.task-runner-manifest.v1).
     - scripts/check-task-runner-manifest.mjs (read-only checker; never shells out, never touches git / network / LLM).

     Schema head: (task-runner-manifest <manifest-id> :schema :wave :brief_mode :shared_preamble_path :productive_only [:overlap_policy :description :generated_at :generator] (node ...) (node ...) ...).
     Required header fields (5): :schema :wave :brief_mode :shared_preamble_path :productive_only.
     Optional header fields (4): :overlap_policy :description :generated_at :generator.
     Required node fields (7): :task_id :depends_on :verification_tier :dispatch_group :estimated_minutes :heartbeat_minutes :write_scope.
     Optional node fields (3): :notes :owner :kind.

     Enums:
       BRIEF_MODES         = {thin | full | preamble}
       VERIFICATION_TIERS  = {local | smoke | full}    (mirrors task-contract v1; drift = structural error)
       OVERLAP_POLICIES    = {reject | warn}            (default = reject)
       FORBIDDEN_PRODUCTIVE_KINDS = {archive | backfill | index | lisp-backfill}

     Literal-atom-only fields (strings rejected):
       :productive_only           = true OR false
       :estimated_minutes         = positive integer atom (^[1-9][0-9]*$; zero / negative / fractional / string rejected)
       :heartbeat_minutes         = positive integer atom (^[1-9][0-9]*$; zero / negative / fractional / string rejected)

     Path fields (no leading '/' or '~', no '..' traversal): :shared_preamble_path, every entry of :write_scope.

     Cross-node rules:
       1. duplicate :task_id within a manifest = error.
       2. :depends_on entry referencing a non-existent node id in the SAME manifest = error.
       3. :depends_on self-edge = error.
       4. write_scope overlap among nodes in the SAME :dispatch_group = error when :overlap_policy=reject (default), warning when :overlap_policy=warn.
       5. :productive_only true with a node whose :kind ∈ FORBIDDEN_PRODUCTIVE_KINDS or whose :task_id contains '-archive-' / '-backfill-' / '-index' / 'lisp-backfill' = error.

     Warnings (advisory, do not fail the checker):
       1. :shared_preamble_path file not present on disk (only when reading from a real file with :brief_mode=thin|preamble).
       2. write_scope overlap inside a dispatch_group when :overlap_policy=warn.

     Checker CLI: node scripts/check-task-runner-manifest.mjs [--json] [--stdin] [--dry-fixture] [<file.lisp> ...].
     JSON shape: { ok, files, manifests_validated, nodes_validated, errors[], warnings[] }.
     --stdin lets wave28-02 pipe its emitted manifest through the checker without a temp file.

     Dry-fixture totals: 20 cases across 15 categories — pass (1: valid productive 2-group), pass-overlap-warn (1), pass-overlap-cross-group (1), fail-duplicate (1), fail-missing-dep (1), fail-self-edge (1), fail-enum (2: verification_tier + brief_mode), fail-overlap (1), fail-positive-int (2: zero heartbeat + negative estimated), fail-string-bool (1: productive_only=\"true\"), fail-productive-only (3: archive id + backfill id + index :kind), fail-path (2: absolute write_scope + traversal preamble), fail-unknown-entry-head (1), fail-unknown-field (1), fail-missing-required (1).

     Named exports for wave28-02 / wave28-03 / wave28-05 import:
       SCHEMA, MANIFEST_HEAD, NODE_HEAD,
       BRIEF_MODES, VERIFICATION_TIERS, OVERLAP_POLICIES, FORBIDDEN_PRODUCTIVE_KINDS,
       projectManifest (form -> structured object),
       readManifestFile (file path -> array of projected manifests),
       validateManifestObject (object -> array of error message strings; empty = valid).
     main() is gated on import.meta.url === `file://${process.argv[1]}` so importers do not accidentally trigger CLI side-effects.

     Dispatch-plan vs manifest distinction: .missiond/tasks/wave28/dispatch-plan.lisp is hand-authored orchestrator policy (productive-only, archive/backfill/index orchestrator-owned). The manifest schema is a complementary machine-emittable surface for wave28-02 (plan CLI), wave28-03 (brief renderer), and wave28-05 (batch verifier). Manifests are advisory orchestration metadata only — NOT a worker-task surface and NOT a runtime backend switch.

     Pre-commit pipeline: check-task-runner-manifest.mjs --dry-fixture (exit=0) → check-task-contract.mjs --all (exit=0, 98 tasks) → git diff --check (exit=0) → check-missiond-hooks.mjs --json (preflight aligned) → git add (2 paths) → task-scope-guard.mjs --mode staged (OK, 2 staged) → MISSIOND_TASK_CONTRACT=... git commit -m \"feat(tasks): add task runner manifest schema\" (commit 145645c2fd5a) → verify-task-contract.mjs (OK against 145645c2fd5a). All append-only ledger updates: shared-memory wave28-01-claim-001 (seq 2) before staging + wave28-01-completion-001 (seq 3) after verify; session-trace wave28-01-trace-start-001 (seq 2) before reading background + wave28-01-trace-commit-001 (seq 3, with commit hash) + wave28-01-trace-complete-001 (seq 4). Both ledgers re-validated after each append.

     Constraints honored: NO Rust / SQL / Cargo edits. Did not touch crates/**, .missiond/v2/**, .missiond/router/**, .missiond/tasks/wave27/**, any wave28-*.lisp other than session-trace + shared-memory (both are session-trace-writable / claim-allowed and explicitly NOT in :must-not-touch), .missiond/tasks/wave28/dispatch-plan.lisp, .missiond/claudecode/**, scripts/render-claudecode-task.mjs, scripts/check-task-contract.mjs, scripts/plan-task-runner.mjs, scripts/render-wave-briefs.mjs, scripts/verify-task-runner-batch.mjs. Did not git add . / git push / --no-verify / --amend / --force.")
