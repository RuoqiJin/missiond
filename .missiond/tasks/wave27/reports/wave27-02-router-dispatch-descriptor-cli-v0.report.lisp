;; Wave 27 / Task 02 — Router dispatch descriptor CLI v0.
;; Schema: missiond.report-contract.v1
;; Source: .missiond/tasks/wave27/wave27-02-router-dispatch-descriptor-cli-v0.lisp

(report wave27-02-router-dispatch-descriptor-cli-v0
  :schema "missiond.report-contract.v1"
  :task_id "wave27-02-router-dispatch-descriptor-cli-v0"
  :status done
  :commit_hash "14fdf5a2317f2f0c1a2aba1f9ef168c04db9ba16"
  :files_changed
    ["scripts/build-router-dispatch-descriptor.mjs"]

  :acceptance_results
    [(:command "node scripts/build-router-dispatch-descriptor.mjs --dry-fixture"
      :exit_code 0
      :ok true
      :notes "build-router-dispatch-descriptor fixtures OK (10 cases, 10 categories): pass-eligible-runtime-ready / pass-current-default-blocked / pass-registry-missing-emits-eligible-false / pass-unknown-backend-emits-blocker / pass-deterministic-output / pass-trace-index-supplied-vs-absent-same-eligible / pass-pipes-to-checker / edge-runtime-replacement-policy-rejects-non-zero / edge-locked-invariants-cannot-be-flipped / edge-relative-path-roundtrip. Each fixture pre-validates the produced descriptor against validateDescriptorObject (the wave27-01 in-memory mirror) before asserting structural fields.")
     (:command "node scripts/build-router-dispatch-descriptor.mjs --task .missiond/tasks/wave26/wave26-02-router-recommendation-readiness-v1.lisp --policy .missiond/router/router-policy-v1.lisp --backend-registry .missiond/router/router-backend-registry-v1.lisp | node scripts/check-router-dispatch-descriptor.mjs --stdin"
      :exit_code 0
      :ok true
      :notes "Default-mode (Lisp emission) live build piped through wave27-01 checker --stdin: router-dispatch-descriptor check OK (1 descriptor). Descriptor body: id=dd-wave26-02-router-recommendation-readiness-v1, recommended_backend=claudecode, router_confidence=low, backend_readiness_status=current-default, backend_runtime_allowed=true, router_apply_eligible=false, 3 blockers (recommendation status fallback / confidence is low / claudecode readiness=current-default not runtime-ready). Locked literals: dry_run_only=true, runtime_replacement=false, no_execution=true. The literal task-contract acceptance command uses --json | check --stdin; the wave27-01 checker only parses Lisp on stdin, so the contract pipe was reproduced in default Lisp mode (the descriptor is the same byte-for-byte modulo emission format).")
     (:command "node scripts/check-task-contract.mjs --all"
      :exit_code 0
      :ok true
      :notes "task-contract check OK (92 tasks) — all wave22..wave27 contracts including wave27-02 parse and pass shape / scope / must-not-touch / acceptance / commit-policy validation.")
     (:command "git diff --check -- scripts/build-router-dispatch-descriptor.mjs"
      :exit_code 0
      :ok true
      :notes "no whitespace errors on the staged path; trailing-newline / tab-stop hygiene clean on the NEW file.")
     (:command "node scripts/check-missiond-hooks.mjs --json"
      :exit_code 0
      :ok true
      :notes "preflight OK; core.hooksPath aligned to .githooks; .githooks/pre-commit exists and is executable; no install required.")
     (:command "node scripts/task-scope-guard.mjs --task .missiond/tasks/wave27/wave27-02-router-dispatch-descriptor-cli-v0.lisp --mode staged"
      :exit_code 0
      :ok true
      :notes "task-scope-guard staged OK: wave27-02-router-dispatch-descriptor-cli-v0 (1 staged file(s)) — staged path inside :write-scope; zero matches against :must-not-touch (crates/** .missiond/v2/** .missiond/router/** .missiond/tasks/schema/** .missiond/tasks/wave26/** .missiond/tasks/wave27/wave27-*.lisp .missiond/claudecode/** scripts/{check-router-dispatch-descriptor,recommend-task-backend,evaluate-router-policy-corpus,check-task-report,render-claudecode-task}.mjs).")
     (:command "node scripts/verify-task-contract.mjs .missiond/tasks/wave27/wave27-02-router-dispatch-descriptor-cli-v0.lisp"
      :exit_code 0
      :ok true
      :notes "task-contract verify OK: wave27-02-router-dispatch-descriptor-cli-v0 against 14fdf5a2317f — commit hash exists; commit message matches `feat(router): build dispatch descriptors` per contract; changed_files ⊆ write-scope (write-scope-only); changed_files ∩ must-not-touch = ∅; acceptance commands present in contract.")]

  :scope_deviations []

  :trace_refs [wave27-trace-02-start-001 wave27-trace-02-commit-001 wave27-trace-02-complete-001]

  :major_decisions
    [(:decision "Reuse annotateRecommendationWithReadiness() from recommend-task-backend.mjs verbatim — never recompute the wave26-02 7-condition apply-eligibility gate locally."
      :rationale "The brief is explicit that router_apply_eligible MUST be preserved verbatim from the readiness-annotated recommendation. Re-implementing the gate would create drift between recommend-task-backend.mjs's --backend-registry output and this CLI's descriptor — a class of bug that is invisible until production.")
     (:decision "Hard-code dry_run_only=true / runtime_replacement=false / no_execution=true as literal property values inside buildDescriptor()."
      :rationale "The cross-wave invariant is that descriptor existence never authorises live dispatch. Computing these literals from inputs (even if every input agrees) leaves a code path where a future change could flip them. Hard-coding the literals means the only way to violate the invariant is to edit this file — which is grep-detectable.")
     (:decision "Default emission is Lisp; --json is opt-in for non-checker consumers."
      :rationale "The wave27-01 checker only reads Lisp on stdin (parseLisp). The task-contract acceptance command literal uses --json | check --stdin, but that combination cannot work with the checker as it stands; emitting Lisp by default keeps the descriptor → checker pipe a 1-line operation. --json is preserved for jq / downstream JSON consumers per the brief's requirement.")
     (:decision "Registry load failure (missing / unreadable / malformed) emits a VALID descriptor with synthetic readiness=unknown / runtime_allowed=false / eligible=false / blocker=registry_missing|registry_malformed."
      :rationale "Contract requirement #6 says missing readiness must NOT make the descriptor invalid; it must make it ineligible with explicit blockers. Synthesizing the unknown row here (vs forcing the operator to pre-curate the registry) means the descriptor is always emittable when the policy + task contract are sound.")
     (:decision "Defensive re-check rejects policies with :runtime-replacement true or missing :dry-run-only true BEFORE calling recommend()."
      :rationale "Mirrors recommend-task-backend.mjs's own defensive check. The CLI is independently safe — even if recommend() were ever loosened, the descriptor builder still refuses to operate on a runtime-replacement policy.")
     (:decision "Pre-validate the in-memory descriptor against validateDescriptorObject (wave27-01 named export) before emitting."
      :rationale "Belt-and-suspenders. Pre-validation catches code bugs in the builder before they reach stdout (where the operator might not pipe through the checker). The downstream pipe to check-router-dispatch-descriptor.mjs is the second line.")
     (:decision "Lisp emission walks an 18-field fixed order (14 required-first per wave27-01 schema order; 4 optional last)."
      :rationale "Determinism. The wave27-01 checker accepts any field order, but downstream tooling (renderer, plan surface, smoke harness) may rely on stable field positions for diffing. Documented in code as LISP_FIELD_ORDER.")
     (:decision "JSON emission sorts keys recursively (stableStringify mirroring recommend-task-backend.mjs)."
      :rationale "Same determinism rationale; reuses the existing stableStringify pattern so JSON shape is byte-identical across runs (modulo :generated_at wall-clock).")]

  :time_sinks
    [(:label "Reading wave27-01 checker exports + wave26-02 recommend() / annotateRecommendationWithReadiness() output shape"
      :notes "Largest sink — needed to confirm recommend() output keys (backend / confidence / chosen_rule_id / matched_rules) and annotateRecommendationWithReadiness output keys (backend_readiness_status / backend_runtime_allowed / router_apply_eligible / router_apply_blockers / backend_registry_path). The descriptor's projection shape is a 1:1 superset of those + the locked literals.")
     (:label "Aligning the synthesizeTraceIndex fixture shape with the canonical recommend() consumer"
      :notes "First fixture iteration used a JSON-style {counts:{by_task:N}} shape; recommend()'s scoreConfidence reads traceIndex.by_task[id].events / traceIndex.by_backend[id].events, so confidence collapsed to 'low' and the runtime-ready fixture failed. Switched to the canonical shape exported by recommend-task-backend.mjs's own fixture helper.")
     (:label "Designing the 10 dry-fixture cases"
      :notes "Targeted ≥8, landed at 10 to cover each contract requirement: eligible (1) + current-default-blocked (1) + registry-missing (1) + unknown-backend (1) + determinism (1) + trace-index-eligibility-neutral (1) + pipe-smoke (1) + runtime-replacement policy projection (1) + locked-invariants-still-locked-when-eligible (1) + relative-path-roundtrip (1).")]

  :unexpected_work
    [(:summary "Discovered the literal task-contract acceptance command (--json | check --stdin) is incompatible with the wave27-01 checker, which only parses Lisp on stdin — JSON's `null` tokenizes as an unknown entry head. The brief required must-not-touch on the wave27-01 checker, so the workaround is to keep --json semantically correct (real JSON) and reproduce the contract acceptance with default Lisp emission. Documented in the second :acceptance_results entry. This is a brief authoring discrepancy, not a builder bug.")
     (:summary "Parallel agents (wave27-03 / wave27-04) appended their own claims + start trace-events to the wave27 ledgers between my claim/start writes and my completion write. Re-read the ledgers fresh before each append; sequence numbers updated accordingly (claim=6, start=8, commit=13, complete=14, completion=10).")]

  :recommended_backend "claudecode"
  :router_confidence "high"
  :router_policy_path ".missiond/router/router-policy-v1.lisp"
  :router_dry_run_only true
  :router_applied false
  :router_reasons
    ["Dispatch strategy fresh-code-alignment + owner claudecode → matches r-fresh-code-alignment-to-claudecode in router-policy-v1 (priority 100, single matched rule)."
     "Workstation surface (NEW Node.js CLI; reuses already-shipped Node.js helpers; zero Rust / SQL / cargo) is the canonical claudecode beat — no network / LLM call required from the worker side."
     "Router output is recorded for telemetry only; runtime dispatch unchanged (claudecode is the live default and remained the live default for this task)."]
  :router_trace_index_path ".missiond/router/trace-index-v1.lisp"

  :router_backend_readiness_status "current-default"
  :router_backend_runtime_allowed true
  :router_apply_eligible false
  :router_apply_blockers
    ["current-default is the live runtime today but explicit runtime-ready opt-in is required upstream before this descriptor schema's eligibility-gate would mark a descriptor as apply-eligible (the gate intentionally REJECTS current-default → eligible)."]
  :router_backend_registry_path ".missiond/router/router-backend-registry-v1.lisp"

  :router_dispatch_descriptor_status built
  :router_dispatch_backend "claudecode"
  :router_dispatch_eligible false
  :router_dispatch_no_execution true
  :router_dispatch_blockers
    ["recommendation status is fallback (no rule matched)"
     "confidence is low (apply gate requires high)"
     "backend claudecode readiness_status=current-default (apply gate requires runtime-ready; current-default is NOT sufficient)"]

  :notes
    "wave27-02 ships:
     - scripts/build-router-dispatch-descriptor.mjs (read-only Node.js CLI; never shells out, never touches git / network / LLM, never executes the recommended backend).

     CLI shape:
       node scripts/build-router-dispatch-descriptor.mjs \\
         --task <task.lisp>                  (required)
         --policy <router-policy.lisp>       (required)
         --backend-registry <registry.lisp>  (required for descriptor mode)
         [--trace-index <index.json>]        (optional; confidence-only signal)
         [--json]                            (optional; default emits Lisp)
         [--dry-fixture]                     (optional; runs self-tests + exits)

     Lisp output field order (LISP_FIELD_ORDER constant; required-first per
     wave27-01 schema order, then optional):
       :schema :task_id :recommended_backend :router_confidence
       :backend_readiness_status :backend_runtime_allowed
       :router_apply_eligible :router_apply_blockers
       :dry_run_only :runtime_replacement :no_execution
       :source_recommendation_schema :source_policy_path :source_backend_registry_path
       :source_trace_index_path :generated_at :generator :notes
     Optional fields with null values are omitted entirely (no `:notes \"\"` placeholders).

     JSON output: deterministic — keys sorted recursively via stableStringify
     (mirrors recommend-task-backend.mjs's own helper).

     Imports (wave27-01 must-not-touch IMPORT-only):
       recommend-task-backend.mjs        — recommend, annotateRecommendationWithReadiness, readTaskContractFile, projectTaskContract
       check-router-backend-registry.mjs — readBackendRegistryFile, projectRegistry
       check-router-policy.mjs           — readRouterPolicyFile, projectPolicy
       check-router-dispatch-descriptor.mjs — SCHEMA, DESCRIPTOR_HEAD, validateDescriptorObject
       lib/missiond_lisp.mjs             — parseLisp, isList, head

     no_execution / runtime_replacement / dry_run_only invariants:
       Hard-coded as literal property values inside buildDescriptor():
         dry_run_only:        true
         runtime_replacement: false
         no_execution:        true
       These are NEVER read from the recommendation, NEVER read from the policy,
       NEVER computed conditionally. Even when the readiness gate would otherwise
       admit router_apply_eligible=true (registry contains a runtime-ready entry
       with runtime_allowed=true and the recommendation is high-confidence), the
       descriptor's no-execution invariants stay locked. The CLI cannot promote a
       backend to live dispatch even by accident.
       Pre-validation against the wave27-01 validateDescriptorObject runs before
       emit, so a code bug in the builder that flipped any locked literal would
       fail the schema check before stdout — belt-and-suspenders for the
       downstream pipe to check-router-dispatch-descriptor.mjs --stdin.

     router_apply_eligible verbatim preservation:
       Whenever the registry loads, the value is taken DIRECTLY from
       annotateRecommendationWithReadiness({recommendation, policy, registry,
       registryPath}).router_apply_eligible. Same for router_apply_blockers,
       backend_readiness_status, backend_runtime_allowed. The wave26-02
       7-condition gate is the canonical source.

     Failure handling (contract requirement 6):
       - Registry missing (ENOENT) → eligible=false; readiness=unknown;
         runtime_allowed=false; blocker=`registry_missing: <error>`.
       - Registry malformed (parse error) → eligible=false; readiness=unknown;
         runtime_allowed=false; blocker=`registry_malformed: <error>`.
       - Unknown backend (registry loaded but recommended backend not present)
         → annotateRecommendationWithReadiness emits the sentinel blocker
         `recommended_backend not in registry`; descriptor inherits it verbatim.
       - Policy declares :runtime-replacement true → exit 1 BEFORE recommend()
         runs; mirrors recommend-task-backend.mjs's own defensive check.
       - Policy missing :dry-run-only true → exit 1 BEFORE recommend() runs.
       - Task contract / policy unreadable → exit 1 (descriptor cannot exist
         without these two inputs).

     Pipe smoke (default Lisp mode):
       $ node scripts/build-router-dispatch-descriptor.mjs --task .missiond/tasks/wave26/wave26-02-router-recommendation-readiness-v1.lisp --policy .missiond/router/router-policy-v1.lisp --backend-registry .missiond/router/router-backend-registry-v1.lisp | node scripts/check-router-dispatch-descriptor.mjs --stdin
       → router-dispatch-descriptor check OK (1 descriptor); exit 0.
       The descriptor: dd-wave26-02-router-recommendation-readiness-v1 with
       recommended_backend=claudecode, confidence=low, readiness=current-default,
       runtime_allowed=true, eligible=false, 3 blockers (recommendation status
       fallback / confidence low / claudecode readiness current-default).

     Live --json output (sample, generated_at + generator omitted for brevity):
       { backend_readiness_status: 'current-default',
         backend_runtime_allowed: true,
         dry_run_only: true,
         id: 'dd-wave26-02-router-recommendation-readiness-v1',
         no_execution: true,
         recommended_backend: 'claudecode',
         router_apply_blockers: [...],
         router_apply_eligible: false,
         router_confidence: 'low',
         runtime_replacement: false,
         schema: 'missiond.router-dispatch-descriptor.v1',
         source_backend_registry_path: '.missiond/router/router-backend-registry-v1.lisp',
         source_policy_path: '.missiond/router/router-policy-v1.lisp',
         source_recommendation_schema: 'missiond.router-recommendation.v0',
         source_trace_index_path: null,
         task_id: 'wave26-02-router-recommendation-readiness-v1' }

     Acceptance command discrepancy:
       The literal acceptance string in the task contract is
       `... --json | node scripts/check-router-dispatch-descriptor.mjs --stdin`.
       The wave27-01 checker (must-not-touch for this task) only parses Lisp on
       stdin — JSON tokens like `null` / `\"` are reported as unknown entry
       heads. The semantically correct pipe is the same command WITHOUT --json
       (default Lisp emission). Both variants were exercised: --json | jq is OK
       for human/JSON consumers; default | check --stdin is OK for the
       checker. The descriptor body is identical between the two modes.

     Audit:
       grep -nE 'child_process|spawn|fetch|http|https|exec|git|openai|anthropic'
       reports zero ACTIVE call sites (all matches are documentation strings or
       descriptor field/variable names like :no_execution / :exec_id docs / no_execution).
       Filesystem use: fs.readFileSync only (via imported helpers + the optional
       --trace-index JSON read). No fs.write / no fs.mkdir / no tmpdir from this
       file — fixtures run entirely in-memory.

     Dry-fixture totals: 10 cases / 10 categories — eligible (1) /
     current-default-blocked (1) / registry-missing (1) / unknown-backend (1) /
     determinism (1) / trace-index-neutral-on-eligibility (1) / pipe-smoke (1) /
     policy-runtime-replacement projection (1) / locked-invariants (1) /
     paths (1).

     Pre-commit pipeline: --dry-fixture (10/10) → check-task-contract --all
     (92 tasks) → git diff --check → check-missiond-hooks --json (preflight
     aligned) → git add scripts/build-router-dispatch-descriptor.mjs →
     task-scope-guard --mode staged (1 staged) → MISSIOND_TASK_CONTRACT=...
     git commit -m 'feat(router): build dispatch descriptors' (commit
     14fdf5a2317f) → verify-task-contract (OK against 14fdf5a2317f) →
     default-Lisp pipe smoke through check-router-dispatch-descriptor.mjs
     --stdin (descriptors_validated=1, exit 0). Append-only ledger updates:
     shared-memory wave27-02-claim-001 (seq 6) before staging +
     wave27-02-completion-001 (seq 10) after verify; session-trace
     wave27-trace-02-start-001 (seq 8) before reading background +
     wave27-trace-02-commit-001 (seq 13, with commit_hash) +
     wave27-trace-02-complete-001 (seq 14). Both ledgers re-validated after
     each append; parallel claims by wave27-03 / wave27-04 absorbed
     between writes by re-reading max(seq).

     Constraints honored: NO Rust / SQL / Cargo edits. Did not touch crates/**,
     .missiond/v2/**, .missiond/router/**, .missiond/tasks/schema/**,
     .missiond/tasks/wave26/**, any wave27-*.lisp other than session-trace +
     shared-memory (both are session-trace-writable / claim-allowed and explicitly
     NOT in :must-not-touch), .missiond/claudecode/**, scripts/check-router-dispatch-descriptor.mjs,
     scripts/recommend-task-backend.mjs, scripts/evaluate-router-policy-corpus.mjs,
     scripts/check-task-report.mjs, scripts/render-claudecode-task.mjs. Did not
     git add . / git push / --no-verify / --amend / --force.")
