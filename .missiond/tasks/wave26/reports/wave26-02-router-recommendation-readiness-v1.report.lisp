;; Wave 26 / Task 02 — Router recommendation readiness annotations v1.
;; Schema: missiond.report-contract.v1
;; Source: .missiond/tasks/wave26/wave26-02-router-recommendation-readiness-v1.lisp

(report wave26-02-router-recommendation-readiness-v1
  :schema "missiond.report-contract.v1"
  :task_id "wave26-02-router-recommendation-readiness-v1"
  :status done
  :commit_hash "ad8ec0467df2"
  :files_changed
    ["scripts/recommend-task-backend.mjs"
     "scripts/evaluate-router-policy-corpus.mjs"]

  :acceptance_results
    [(:command "node scripts/recommend-task-backend.mjs --dry-fixture"
      :exit_code 0
      :ok true
      :notes "recommend-task-backend fixtures OK (17 cases, 17 categories). 12 wave22-25 baseline fixtures pass byte-identically + 5 new wave26-02 categories: wave26-readiness-eligible (positive control: 7-condition gate true → router_apply_eligible=true), wave26-readiness-blocked-current-default (seed-shape registry → false; blocker explicitly mentions current-default and runtime-ready opt-in), wave26-readiness-blocked-confidence-medium (matched + runtime-ready but medium → false; blocker mentions confidence), wave26-readiness-unknown-backend (registry exists but recommended backend absent → status=unknown; first blocker is sentinel 'recommended_backend not in registry'), wave26-readiness-without-registry-flag (no annotate call → output keys exactly match wave25 baseline set, JSON contains zero new field names).")
     (:command "node scripts/evaluate-router-policy-corpus.mjs --dry-fixture"
      :exit_code 0
      :ok true
      :notes "evaluate-router-policy-corpus fixtures OK (11 cases, 11 categories). 9 wave22-25 baseline fixtures pass byte-identically + 2 new wave26-02 categories: wave26-corpus-with-registry (3-task corpus + synthetic registry where claudecode is runtime-ready → by_backend_readiness {runtime-ready:1, advisory-only:2}, apply_eligible_count=1; per_task row keys remain {backend, confidence, fallback_reason, matched_rule_id, task_id, task_path} — no row-level drift), wave26-corpus-without-registry (no flag → top-level keys exactly the wave25 baseline set, no by_backend_readiness, no apply_eligible_count).")
     (:command "node scripts/recommend-task-backend.mjs --task .missiond/tasks/wave26/wave26-01-router-backend-registry-v0.lisp --policy .missiond/router/router-policy-v1.lisp --backend-registry .missiond/router/router-backend-registry-v1.lisp --json"
      :exit_code 0
      :ok true
      :notes "JSON recommendation surfaces all 5 new fields. The wave26-01 task is code-alignment + scripts/check-* so r-deterministic-checker-tasks matches. backend=deterministic-checker, confidence=low (no trace history), backend_readiness_status=advisory-only, backend_runtime_allowed=false, router_apply_eligible=false, router_apply_blockers=[seed apply_blockers (2) + 'confidence is low (apply gate requires high)' + 'backend deterministic-checker runtime_allowed=false in registry' + 'backend deterministic-checker readiness_status=advisory-only (apply gate requires runtime-ready; current-default is NOT sufficient)']. backend_registry_path echoed verbatim. dry_run_only=true literal preserved.")
     (:command "node scripts/evaluate-router-policy-corpus.mjs --policy .missiond/router/router-policy-v1.lisp --tasks-root .missiond/tasks --backend-registry .missiond/router/router-backend-registry-v1.lisp --json"
      :exit_code 0
      :ok true
      :notes "Real-corpus run across 75 task contracts (.missiond/tasks/wave22..wave26). by_backend {claudecode:54, deterministic-checker:16, missiond-llm-router:0, patch-worker:0, verifier-worker:5}. by_backend_readiness {current-default:54, advisory-only:21}. by_confidence {high:6, medium:7, low:62}. apply_eligible_count=0 — exactly as the brief predicts: no backend in the seed registry declares :readiness_status runtime-ready (claudecode is current-default), so the strict 7-condition gate rejects every task. fallback_count=48 (tasks with no rule match). rejected_count=0. trace_index_source=built-in-process:.missiond/tasks. The zero apply_eligible_count proves the apply gate is dormant under the seed shape, confirming the cross-wave invariant: even the live default backend (claudecode) is NOT auto-promoted by the gate; an explicit runtime-ready opt-in must ship before any task becomes apply-eligible.")
     (:command "node scripts/check-task-contract.mjs --all"
      :exit_code 0
      :ok true
      :notes "task-contract check OK (83 tasks). All contracts continue to parse and pass shape / scope / must-not-touch / acceptance / commit-policy validation; no drift introduced by the script edits.")
     (:command "git diff --check -- scripts/recommend-task-backend.mjs scripts/evaluate-router-policy-corpus.mjs"
      :exit_code 0
      :ok true
      :notes "git diff --check exit 0 — no whitespace or conflict-marker errors in either edited file.")]

  :time_sinks
    ["read recommend-task-backend.mjs end-to-end (1365 lines, 12 dry-fixture cases) to map the recommendation flow + identify the right anchor for the readiness annotation (post-recommend in main(), separate helper for fixture reuse)"
     "read evaluate-router-policy-corpus.mjs end-to-end (1114 lines, 9 dry-fixture cases) to map the evaluator flow + plan how to add aggregates without touching per_task row shape (backward-compat for wave25-02 report consumers)"
     "audit check-router-backend-registry.mjs named exports to confirm exact symbol names (SCHEMA, REGISTRY_HEAD, BACKEND_HEAD, BACKEND_IDS, READINESS_STATUSES, RUNTIME_ALLOWED_STATUSES, projectBackend, projectRegistry, readBackendRegistryFile) and re-confirm the gating contract (CLI on import.meta.url === file://process.argv[1])"
     "design the strict 7-condition apply-eligible gate to reject seed-shape claudecode (current-default) explicitly — the brief's most subtle requirement: 'current-default is NOT sufficient; explicit runtime-ready opt-in required'. Documented in three places (CLI doc-comment, helper doc-comment, blocker string)"
     "wire 5 wave26-02 fixtures + 2 corpus fixtures driving the registry parse via dynamic import of REGISTRY_HEAD/projectRegistry to keep the wave25 baseline call graph free of the registry module"]

  :major_decisions
    [(:decision "Annotation lives in a separate helper annotateRecommendationWithReadiness() rather than mutating recommend()."
      :rationale "Keeps recommend() byte-identical for wave25 baseline consumers (corpus evaluator, plan handler, downstream tooling). The helper takes the base recommendation + policy + registry + registryPath and returns a NEW object with 5 additive fields — input is not mutated.")
     (:decision "Lazy dynamic import of check-router-backend-registry.mjs gated on --backend-registry flag in BOTH CLIs."
      :rationale "Honors the brief's explicit 'lazy-import the registry module so no I/O occurs' contract. When --backend-registry is absent, the registry module is never loaded, the registry helper is never called, and the wave25 baseline call graph is preserved bit-for-bit. main() became async to support this.")
     (:decision "router_apply_eligible requires explicit readiness_status === 'runtime-ready', NOT 'current-default'."
      :rationale "The brief's most load-bearing rule: with the seed registry where claudecode is current-default + runtime_allowed=true + 0 blockers, the gate STILL rejects. This forces a future apply gate to see an explicit runtime-ready opt-in beyond the historical default — preserves the cross-wave invariant that promotion is a deliberate per-backend decision, not an accidental side-effect of being the live default. Documented in the CLI doc-comment, the annotateRecommendationWithReadiness helper doc-comment, the blocker string itself ('apply gate requires runtime-ready; current-default is NOT sufficient'), and the wave26-readiness-blocked-current-default fixture.")
     (:decision "Corpus evaluator aggregates at the top level only; per_task rows retain wave25 shape."
      :rationale "Wave25-02 already pinned per_task row keys {backend, confidence, fallback_reason, matched_rule_id, task_id, task_path}. Adding per-row readiness fields would break that contract. Instead the evaluator runs annotateRecommendationWithReadiness internally for aggregation only, then drops the annotated record before constructing the per_task row. Verified by the wave26-corpus-with-registry fixture's row-key assertion.")
     (:decision "router_apply_blockers always carries the registry's apply_blockers verbatim plus explicit gate-failure strings."
      :rationale "Reviewers must be able to read the rejection reason directly off a single field. Verbatim copy preserves the registry author's intent; appended gate strings (confidence/runtime_allowed/readiness_status/policy/status) make the rejection grep-able from the JSON output without re-running the CLI. The unknown-backend case prepends a sentinel string 'recommended_backend not in registry' before any gate strings so the simplest failure mode is the first thing reviewers see.")]

  :unexpected_work
    ["evaluate-router-policy-corpus.mjs did not previously import the missiond_lisp helpers (only imported via recommend-task-backend.mjs); had to add `import { head, isList, parseLisp } from './lib/missiond_lisp.mjs'` to support the dry-fixture parseRegistryFromString helper. The production code path does not use parseLisp directly — only the fixtures do."
     "Real-corpus walk found 75 tasks (not 67 as the brief estimated) — the corpus has grown since wave26-01. apply_eligible_count=0 still holds; the count expectation was correct, the cardinality estimate was outdated."]

  :blockers []

  :trace_refs
    ["wave26-02-trace-start-001"
     "wave26-02-trace-commit-001"
     "wave26-02-trace-complete-001"
     ".missiond/tasks/wave26/session-trace.lisp"]

  :notes "Backward-compat is non-negotiable and verified three ways: (1) baseline fixtures pass unchanged in both CLIs (12+9 byte-identical); (2) dedicated wave26-02 fixtures wave26-readiness-without-registry-flag and wave26-corpus-without-registry-omits-fields assert the absence of the new field names in baseline output AND the exact wave25 top-level key set; (3) explicit grep on real CLI output without --backend-registry confirms zero hits for any of the 7 new field names. Audit: zero new active call sites for child_process / spawn / fetch / http / exec / git / openai / anthropic in either edited file (existing self-audit fixture in evaluate-router-policy-corpus.mjs still passes after the edits — strips comments + string literals before scanning). The recommend-task-backend.mjs side does not have a self-audit fixture today; the only forbidden-pattern hits in the file are inside comments and the wave24-06 audit fixture's own pattern table, both of which are excluded by comment / string stripping conventions."

  :follow_ups
    ["wave26-03 (parallel agent, claim seq=7) will mirror this 7-condition gate in pure-Rust mission_plan dry_run path, surfacing 6 additive fields on router_recommendation. The Node and Rust implementations MUST agree byte-for-byte on apply_eligible verdict for any given (task, policy, registry, trace-index) tuple — wave26-04 (report contract additive fields) and wave26-06 (smoke fixture) will pin that parity."
     "Future apply gate (post-wave26) will read router_apply_eligible directly. Today no consumer exists; the field is advisory metadata only. The strict gate ensures that when an apply gate ships, it cannot accidentally promote claudecode current-default — a future task must explicitly opt claudecode into runtime-ready first."])
