;; Wave 27 / Task 01 — Router dispatch descriptor schema v0.
;; Schema: missiond.report-contract.v1
;; Source: .missiond/tasks/wave27/wave27-01-router-dispatch-descriptor-schema-v0.lisp

(report wave27-01-router-dispatch-descriptor-schema-v0
  :schema "missiond.report-contract.v1"
  :task_id "wave27-01-router-dispatch-descriptor-schema-v0"
  :status done
  :commit_hash "f451b044abf37f36ce6353f5c4c21f0a8eb18c97"
  :files_changed
    [".missiond/tasks/schema/router-dispatch-descriptor-v1.lisp"
     "scripts/check-router-dispatch-descriptor.mjs"]

  :acceptance_results
    [(:command "node scripts/check-router-dispatch-descriptor.mjs --dry-fixture"
      :exit_code 0
      :ok true
      :notes "router-dispatch-descriptor fixtures OK (22 cases, 9 categories): pass / fail-string-bool / fail-missing-required / fail-locked-invariant / fail-eligibility-gate / fail-enum / fail-path / fail-blockers-shape / fail-unknown-entry-head. JSON variant verified separately and reports the same totals.")
     (:command "node scripts/check-task-contract.mjs --all"
      :exit_code 0
      :ok true
      :notes "task-contract check OK (92 tasks) — all wave22..wave27 task contracts including this one parse and pass shape / scope / must-not-touch / acceptance / commit-policy validation. No regressions vs the wave27-00 baseline (also 92).")
     (:command "git diff --check -- .missiond/tasks/schema/router-dispatch-descriptor-v1.lisp scripts/check-router-dispatch-descriptor.mjs"
      :exit_code 0
      :ok true
      :notes "no whitespace errors on either staged path; trailing-newline / tab-stop hygiene clean on both NEW files.")
     (:command "node scripts/check-missiond-hooks.mjs --json"
      :exit_code 0
      :ok true
      :notes "preflight OK; core.hooksPath aligned to .githooks; .githooks/pre-commit exists and is executable; no install required.")
     (:command "node scripts/task-scope-guard.mjs --task .missiond/tasks/wave27/wave27-01-router-dispatch-descriptor-schema-v0.lisp --mode staged"
      :exit_code 0
      :ok true
      :notes "task-scope-guard staged OK: wave27-01-router-dispatch-descriptor-schema-v0 (2 staged file(s)) — both staged paths inside :write-scope; zero matches against :must-not-touch (crates/** scripts/{recommend-task-backend,evaluate-router-policy-corpus,check-task-report,render-claudecode-task}.mjs .missiond/v2/** .missiond/router/** .missiond/tasks/wave26/** .missiond/tasks/wave27/wave27-*.lisp .missiond/claudecode/**).")
     (:command "node scripts/verify-task-contract.mjs .missiond/tasks/wave27/wave27-01-router-dispatch-descriptor-schema-v0.lisp"
      :exit_code 0
      :ok true
      :notes "task-contract verify OK: wave27-01-router-dispatch-descriptor-schema-v0 against f451b044abf3 — commit hash exists; commit message matches `feat(router): add dispatch descriptor schema` per contract; changed_files ⊆ write-scope (write-scope-only); changed_files ∩ must-not-touch = ∅; acceptance commands present in contract.")
     (:command "echo '<descriptor>' | node scripts/check-router-dispatch-descriptor.mjs --stdin --json"
      :exit_code 0
      :ok true
      :notes "stdin smoke: piped a current-default + apply_eligible=false + non-empty blockers descriptor; checker returned { ok:true, files:1, descriptors_validated:1, errors:[] }. Confirms wave27-02 will be able to pipe the emitted descriptor through this checker without a temp file.")]

  :scope_deviations []

  :trace_refs [wave27-trace-01-start-001 wave27-trace-01-commit-001 wave27-trace-01-complete-001]

  :major_decisions
    [(:decision "Top-level form named (router-dispatch-descriptor ...) per the brief — DESCRIPTOR shape, not a POLICY shape."
      :rationale "The brief is explicit: a descriptor is a single handoff record, not a list of rules. Using a *-descriptor head (vs *-policy / *-registry) keeps reader-side dispatch unambiguous when wave27-02..06 grep for the head atom.")
     (:decision "Lock the three no-execution invariants to single legal values inside the schema itself (not just the checker)."
      :rationale "Encoding the locked values in (locked-invariants ...) inside the schema file makes the cross-wave invariant readable to humans and reduces the temptation to relax it in a later wave by editing only the checker. The checker enforces the same locks for runtime safety.")
     (:decision "Add `unknown` to the 5-element readiness-statuses enum (registry has 4)."
      :rationale "wave26-02 / wave26-03 emit a synthetic `unknown` fallback when the registry lookup fails. Without it in the descriptor enum the checker would reject every miss-fallback descriptor wave27-02 can produce. wave26-04 report-contract already accepts unknown for the same reason.")
     (:decision "APPLY_ELIGIBLE_STATUSES = {runtime-ready} only (current-default REJECTED)."
      :rationale "current-default is the live runtime today, but promoting any backend (or asserting runtime apply-eligibility for the descriptor record itself) requires explicit runtime-ready opt-in upstream — not just 'this is what is running today.' Treating current-default as eligible would let a descriptor silently authorise live dispatch via inertia. The brief is explicit on this rejection.")
     (:decision "router_apply_eligible=false REQUIRES non-empty router_apply_blockers."
      :rationale "An empty blockers vector with eligible=false carries no signal — there is no reason recorded for why apply was rejected. This mirrors the wave26-01 advisory-only / unavailable rule (must list at least one concrete blocker).")
     (:decision "Named exports include validateDescriptorObject (object-shape mirror of the on-disk validator)."
      :rationale "wave27-02 will emit descriptors as in-memory objects from the recommendation join CLI before serializing them. Giving wave27-02 a single import that runs the same enum / locked-invariant / eligibility-gate rules eliminates duplication and keeps the checker as the single source of truth for descriptor shape.")]

  :time_sinks
    [(:label "Reading background schemas + checkers (router-policy / router-backend-registry / report-contract)"
      :notes "Largest sink — 4 files were load-bearing for the design. router-backend-registry was the closest analog (5-element backend-id enum, locked invariants pattern, named exports, gated main). report-contract supplied the optional/required field declaration pattern and the literal-atom-bool helper precedent.")
     (:label "Designing the locked-invariants + eligibility-gate cross-check"
      :notes "Two intertwined rules: 3 single-value locked atoms (not relaxable) plus a 4-condition AND gate (status=runtime-ready, runtime_allowed=true, confidence=high, blockers=[]). Easier to write the schema once both were enumerated; checker code follows the schema layout 1:1.")
     (:label "Drafting 22 dry-fixture cases"
      :notes "Targeted ≥14, landed at 22 to cover each rejection rule plus 3 happy-path variants (eligible-runtime-ready / current-default-blocked / advisory-only-blocked-with-optional-fields). The optional-fields pass case doubles as exercise of :source_trace_index_path / :generated_at / :generator / :notes shape.")]

  :unexpected_work
    [(:summary "Added APPLY_ELIGIBLE_STATUSES as a 1-element set (instead of inlining 'runtime-ready' as a string literal) to keep the eligibility-gate rule self-documenting and easier to audit. Listed in named exports for wave27-02..06 import.")
     (:summary "Schema also exports `unknown` as a fifth readiness status (vs the registry's 4) plus a status-meaning entry explaining it is the wave26-02/03 synthetic fallback. Without this, descriptors emitted from a registry-lookup-miss code path would be unlinkable.")]

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
    ["current-default is the live runtime today but explicit runtime-ready opt-in is required upstream before this descriptor schema's eligibility-gate would mark a descriptor as apply-eligible (the gate intentionally REJECTS current-default → eligible)."]
  :router_backend_registry_path ".missiond/router/router-backend-registry-v1.lisp"

  :notes
    "wave27-01 ships:
     - .missiond/tasks/schema/router-dispatch-descriptor-v1.lisp (schema id missiond.router-dispatch-descriptor.v1).
     - scripts/check-router-dispatch-descriptor.mjs (read-only checker; never shells out, never touches git / network / LLM).

     Schema head: (router-dispatch-descriptor <descriptor-id> ...). Required fields (13): :schema :task_id :recommended_backend :router_confidence :backend_readiness_status :backend_runtime_allowed :router_apply_eligible :router_apply_blockers :dry_run_only :runtime_replacement :no_execution :source_recommendation_schema :source_policy_path :source_backend_registry_path. Optional fields (4): :source_trace_index_path :notes :generated_at :generator.

     Locked invariants (literal atoms only, no string forms accepted):
       :dry_run_only           = true   (LOCKED — descriptor existence never authorises live dispatch)
       :runtime_replacement    = false  (LOCKED — runtime backend replacement is out of scope through wave 27)
       :no_execution           = true   (LOCKED — emitting / consuming a descriptor MUST NOT execute the recommended backend)
     Free-polarity literal booleans (true OR false, but always an atom — strings rejected):
       :backend_runtime_allowed
       :router_apply_eligible

     Eligibility-gate: :router_apply_eligible true REQUIRES ALL of:
       1. :backend_readiness_status = runtime-ready (current-default REJECTED — explicit opt-in required)
       2. :backend_runtime_allowed = true (literal atom)
       3. :router_confidence = high
       4. :router_apply_blockers = () (empty)
     :router_apply_eligible false ALWAYS legal provided :router_apply_blockers is non-empty (zero blockers + eligible=false is a templating bug and is rejected).

     Enums (mirror the upstream wave24-01 / wave26-01 enums; drift = structural error):
       BACKEND_IDS         = {claudecode | missiond-llm-router | deterministic-checker | patch-worker | verifier-worker}
       CONFIDENCE_LEVELS   = {high | medium | low}
       READINESS_STATUSES  = {current-default | advisory-only | runtime-ready | unavailable | unknown}
                             (registry's 4 + the synthetic `unknown` wave26-02/03 emit when the registry lookup fails)
       APPLY_ELIGIBLE_STATUSES = {runtime-ready}    (1-element — current-default REJECTED)

     Path fields (no leading '/' or '~', no '..' traversal): :source_policy_path, :source_backend_registry_path, :source_trace_index_path.

     Checker CLI: node scripts/check-router-dispatch-descriptor.mjs [--json] [--stdin] [--dry-fixture] [<file.lisp> ...]. --stdin lets wave27-02 pipe its emitted descriptor through the checker without a temp file. JSON shape: { ok, files, descriptors_validated, errors[] }.

     Dry-fixture totals: 22 cases across 9 categories — pass (3: eligible-runtime-ready, current-default-blocked, advisory-only-blocked-with-optional-fields), fail-string-bool (2), fail-missing-required (2), fail-locked-invariant (2: runtime_replacement=true, dry_run_only=false), fail-eligibility-gate (5: current-default eligible, medium confidence eligible, non-empty blockers eligible, runtime_allowed=false eligible, eligible=false with empty blockers), fail-enum (3: unknown backend / readiness / confidence), fail-path (3: absolute / ~ / .. traversal across all 3 path fields), fail-blockers-shape (1: empty string in vector), fail-unknown-entry-head (1).

     Named exports for wave27-02 / 03 / 04 / 05 / 06 import:
       SCHEMA, DESCRIPTOR_HEAD, BACKEND_IDS, CONFIDENCE_LEVELS, READINESS_STATUSES, APPLY_ELIGIBLE_STATUSES,
       projectDescriptor (form -> structured object),
       readDispatchDescriptorFile (file path -> array of projected descriptors),
       validateDescriptorObject (object -> array of error message strings; empty = valid).
     main() is gated on import.meta.url === `file://${process.argv[1]}` so importers do not accidentally trigger CLI side-effects.

     Pre-commit pipeline: check-router-dispatch-descriptor.mjs --dry-fixture (exit=0) → check-task-contract.mjs --all (exit=0) → git diff --check (exit=0) → check-missiond-hooks.mjs --json (preflight aligned) → git add (2 paths) → task-scope-guard.mjs --mode staged (OK, 2 staged) → MISSIOND_TASK_CONTRACT=... git commit -m \"feat(router): add dispatch descriptor schema\" (commit f451b044abf3) → verify-task-contract.mjs (OK against f451b044abf3) → stdin smoke pipe (descriptors_validated=1, ok=true). All append-only ledger updates: shared-memory wave27-01-claim-001 (seq 4) before staging + wave27-01-completion-001 (seq 5) after verify; session-trace wave27-trace-01-start-001 (seq 5) before reading background + wave27-trace-01-commit-001 (seq 6, with commit_hash) + wave27-trace-01-complete-001 (seq 7). Both ledgers re-validated after each append.

     Constraints honored: NO Rust / SQL / Cargo edits. Did not touch crates/**, .missiond/v2/**, .missiond/router/**, .missiond/tasks/wave26/**, any wave27-*.lisp other than session-trace + shared-memory (both are session-trace-writable / claim-allowed and explicitly NOT in :must-not-touch), .missiond/claudecode/**, scripts/recommend-task-backend.mjs, scripts/evaluate-router-policy-corpus.mjs, scripts/check-task-report.mjs, scripts/render-claudecode-task.mjs. Did not git add . / git push / --no-verify / --amend / --force.")
