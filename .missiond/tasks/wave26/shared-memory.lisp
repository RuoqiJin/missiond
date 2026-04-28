;; Wave 26 shared-memory ledger.
;; Schema: .missiond/tasks/schema/shared-memory-v1.lisp
;;
;; Append-only. Agents add entries while they hold a live :claim for their
;; task id. Editing or removing prior entries is forbidden; append a
;; (correction ...) entry instead.

(shared-memory wave26
  :schema "missiond.shared-memory.v1"
  :wave wave26
  :created-at "2026-04-28T18:30:00+08:00"
  :sequence 1

  (observation
    :id wave26-bootstrap-001
    :task wave26-00-archive-wave25-artifacts
    :agent codex-orchestrator
    :seq 1
    :at "2026-04-28T18:30:00+08:00"
    :touched []
    :summary "Bootstrap entry: Wave 26 keeps router-policy dry-run/advisory, but adds a backend readiness registry so recommendations can expose runtime readiness and apply blockers before any future apply gate. No runtime backend replacement task is included.")

  (claim
    :id wave26-00-claim-001
    :task wave26-00-archive-wave25-artifacts
    :agent claudecode
    :seq 2
    :at "2026-04-28T19:00:00+08:00"
    :touched [".missiond/tasks/wave25/**"
              ".missiond/claudecode/wave25-*.md"]
    :summary "Claim wave26-00-archive-wave25-artifacts: archive untracked Wave 25 task contracts (8), rendered briefs (8), reports (6 — wave25-06 lisp backfill and wave25-07 coordination index intentionally have no report), plus shared-memory.lisp and session-trace.lisp ledgers. Verbatim archival commit; no content modifications. wave25-06 lisp backfill committed as 3310b22 satisfying depends-on. write-scope strictly .missiond/tasks/wave25/** and .missiond/claudecode/wave25-*.md.")

  (completion
    :id wave26-00-completion-001
    :task wave26-00-archive-wave25-artifacts
    :agent claudecode
    :seq 3
    :at "2026-04-28T19:06:00+08:00"
    :touched [".missiond/tasks/wave25/**"
              ".missiond/claudecode/wave25-*.md"]
    :summary "Committed wave26-00 in 72d57f83b88b (chore(wave25): archive router measurement artifacts). 24 files archived verbatim: 8 task contracts (wave25-00..wave25-07), 8 rendered briefs (wave25-00..wave25-07), 6 reports (wave25-00..wave25-05; wave25-06 Codex-owned lisp backfill and wave25-07 coordination index intentionally have no report), shared-memory.lisp (13 entries), session-trace.lisp (19 events). Acceptance — check-task-contract --all: exit 0 (83 tasks); check-task-memory wave25/shared-memory.lisp: exit 0 (1 ledger, 13 entries); check-session-trace wave25/session-trace.lisp: exit 0 (1 trace, 19 events); git diff --cached --name-only: 24 paths, all inside write-scope; git diff --check: clean. task-scope-guard --mode staged: 24 staged file(s) OK with 0 must-not-touch matches. verify-task-contract: OK against 72d57f83b88b. check-missiond-hooks: severity ok, reason aligned.")

  (claim
    :id wave26-01-claim-001
    :task wave26-01-router-backend-registry-v0
    :agent claudecode
    :seq 4
    :at "2026-04-28T19:30:00+08:00"
    :touched [".missiond/tasks/schema/router-backend-registry-v1.lisp"
              ".missiond/router/router-backend-registry-v1.lisp"
              "scripts/check-router-backend-registry.mjs"]
    :summary "Claim wave26-01-router-backend-registry-v0: introduce machine-checkable backend readiness registry (schema + seed + checker only — no runtime consumer). Backend ids align with router-policy enum (claudecode, missiond-llm-router, deterministic-checker, patch-worker, verifier-worker). Seed marks claudecode current-default + runtime_allowed true; non-claudecode entries advisory-only or unavailable with apply_blockers populated. Checker mirrors check-router-policy CLI shape with --json / --dry-fixture flags and exports SCHEMA / READINESS_STATUSES / BACKEND_IDS / readBackendRegistryFile for wave26-02/03. Write scope strictly the 3 NEW files.")

  (completion
    :id wave26-01-completion-001
    :task wave26-01-router-backend-registry-v0
    :agent claudecode
    :seq 5
    :at "2026-04-28T19:56:00+08:00"
    :touched [".missiond/tasks/schema/router-backend-registry-v1.lisp"
              ".missiond/router/router-backend-registry-v1.lisp"
              "scripts/check-router-backend-registry.mjs"]
    :summary "Committed wave26-01 in 2ac6a5b2dbd2 (feat(router): add backend readiness registry). 3 files / 1239 insertions inside write-scope. Schema entry head = (backend ...) with required :id :readiness_status :runtime_allowed :apply_blockers :substrate :non-goals (optional :notes :owner :adapter_path). Seed: claudecode current-default + runtime_allowed true + substrate missiond-daemon::handlers::knowledge::workstation_dispatch + 0 apply_blockers; missiond-llm-router / deterministic-checker / verifier-worker advisory-only + runtime_allowed false + nil substrate + 2 apply_blockers each; patch-worker unavailable + runtime_allowed false + nil substrate + 3 apply_blockers. Checker dry-fixture: 19 cases across 13 categories (pass + each rejection rule incl. unknown id, duplicate id, status enum, runtime_allowed literal, runtime_allowed/status mismatch, runtime-ready without substrate, advisory/unavailable with empty blockers, empty non-goals, unknown entry head, missing required field, unknown backend field, schema mismatch). Acceptance — check-router-backend-registry --dry-fixture: exit 0; check-router-backend-registry seed: exit 0 (1 registry, 5 backends); check-task-contract --all: exit 0 (83 tasks); git diff --check: exit 0. task-scope-guard --mode staged: 3 staged file(s) OK with 0 must-not-touch matches. verify-task-contract: OK against 2ac6a5b2dbd2. check-missiond-hooks: severity ok, reason aligned. Codebase audit: only ClaudeCode workstation orchestration is wired in handlers::knowledge::workstation_dispatch / agent_execution today (no adapter for missiond-llm-router, deterministic-checker, patch-worker, or verifier-worker), informing seed apply_blockers text. Named exports for wave26-02/03: SCHEMA, BACKEND_IDS, READINESS_STATUSES, RUNTIME_ALLOWED_STATUSES, REGISTRY_HEAD, BACKEND_HEAD, projectBackend, projectRegistry, readBackendRegistryFile. CLI gated on direct invocation (import.meta.url === file://process.argv[1]) per wave23-06 pattern.")

  (claim
    :id wave26-02-claim-001
    :task wave26-02-router-recommendation-readiness-v1
    :agent claudecode
    :seq 6
    :at "2026-04-28T09:34:00+08:00"
    :touched ["scripts/recommend-task-backend.mjs"
              "scripts/evaluate-router-policy-corpus.mjs"]
    :summary "Claim wave26-02-router-recommendation-readiness-v1: teach recommend-task-backend.mjs and evaluate-router-policy-corpus.mjs to consume the wave26-01 backend readiness registry via in-process import (readBackendRegistryFile / projectRegistry from check-router-backend-registry.mjs). Add optional --backend-registry <path> flag; when supplied, recommendation gains 5 additive fields (backend_readiness_status, backend_runtime_allowed, router_apply_eligible, router_apply_blockers, backend_registry_path) and corpus evaluator gains 2 (by_backend_readiness, apply_eligible_count). Strict 7-condition gate on router_apply_eligible: policy valid + status computed + confidence high + backend in registry + runtime_allowed=true + readiness_status=runtime-ready + apply_blockers empty. current-default alone is NOT sufficient — explicit runtime-ready opt-in required. Backward-compat non-negotiable: outputs byte-identical to wave25 baseline when --backend-registry absent. Write scope strictly the 2 named scripts.")

  (claim
    :id wave26-03-claim-001
    :task wave26-03-plan-router-backend-readiness-v1
    :agent claudecode
    :seq 7
    :at "2026-04-28T09:35:00+08:00"
    :touched ["crates/missiond-daemon/src/handlers/knowledge/plan.rs"
              "crates/missiond-mcp/src/tools/knowledge/plan.rs"]
    :summary "Claim wave26-03-plan-router-backend-readiness-v1: extend mission_plan(router_policy_mode=dry_run) to optionally consume the wave26-01 backend readiness registry via pure-Rust file read + minimal Lisp parser (no Node spawn, no shell). Add optional MCP arg router_backend_registry_path; when supplied AND mode=dry_run, surface 6 additive fields under router_recommendation: backend_registry_path, backend_registry_status (used|missing|unreadable|malformed|unknown_backend), backend_readiness_status (current-default|advisory-only|runtime-ready|unavailable|unknown), backend_runtime_allowed (bool), router_apply_eligible (bool), router_apply_blockers (Vec<String>). Strict 6-condition apply-eligible gate: policy valid (status=computed) + confidence=high + backend in registry + runtime_allowed=true + readiness_status=runtime-ready + apply_blockers empty. current-default alone is NOT sufficient — explicit runtime-ready opt-in required. Off/default mode MUST remain byte-identical with no file I/O even when both router_backend_registry_path AND router_policy_trace_index_path supplied. applied=false stays a hard-coded literal. Dispatch never changes. Write scope strictly the 2 named files.")

  (completion
    :id wave26-02-completion-001
    :task wave26-02-router-recommendation-readiness-v1
    :agent claudecode
    :seq 8
    :at "2026-04-28T09:56:00+08:00"
    :touched ["scripts/recommend-task-backend.mjs"
              "scripts/evaluate-router-policy-corpus.mjs"]
    :summary "Committed wave26-02 in ad8ec0467df2 (feat(router): annotate recommendations with backend readiness). 2 files / 855 insertions / 32 deletions inside write-scope. recommend-task-backend.mjs gains --backend-registry flag + annotateRecommendationWithReadiness({recommendation,policy,registry,registryPath}) helper attaching 5 additive fields (backend_readiness_status / backend_runtime_allowed / router_apply_eligible / router_apply_blockers / backend_registry_path) when registry supplied. evaluate-router-policy-corpus.mjs gains --backend-registry flag + 2 additive top-level aggregates (by_backend_readiness map, apply_eligible_count int) when registry supplied. Strict 7-condition apply-eligible gate: (1) policy.runtime_replacement=false (2) policy.dry_run_only=true (3) chosen_rule_id!==null (status=computed) (4) confidence=high (5) backend in registry (6) runtime_allowed=true (7) readiness_status=runtime-ready AND apply_blockers empty. current-default NOT sufficient — explicit runtime-ready opt-in required (documented in code comments). Lazy dynamic import of check-router-backend-registry.mjs gated on flag presence preserves wave25 baseline call graph. Acceptance — recommend dry-fixture: exit 0 (17/17, +5 wave26 cases); corpus dry-fixture: exit 0 (11/11, +2 wave26 cases); recommend wave26-01 task + seed registry --json: exit 0 (router_apply_eligible=false; deterministic-checker advisory-only with 5 explicit blockers including 'apply gate requires runtime-ready; current-default is NOT sufficient'); corpus full --policy v1 --tasks-root .missiond/tasks --backend-registry seed --json: exit 0 (75 tasks; by_backend_readiness {current-default:54, advisory-only:21}; apply_eligible_count=0 as expected since seed claudecode is current-default); check-task-contract --all: exit 0 (83 tasks); git diff --check: exit 0; check-missiond-hooks: aligned. Backward-compat verified by dedicated fixtures (wave26-readiness-without-registry-flag and wave26-corpus-without-registry-omits-fields) plus explicit grep on no-flag JSON output (zero hits for new field names). Audit: zero new active call sites for child_process/spawn/fetch/http/exec/git/openai/anthropic in either CLI (all hits in comments / strings / pre-existing patterns). task-scope-guard --mode staged: 2 staged file(s) OK with 0 must-not-touch matches. verify-task-contract: OK against ad8ec0467df2.")

  (completion
    :id wave26-03-completion-001
    :task wave26-03-plan-router-backend-readiness-v1
    :agent claudecode
    :seq 9
    :at "2026-04-28T10:11:00+08:00"
    :touched ["crates/missiond-daemon/src/handlers/knowledge/plan.rs"
              "crates/missiond-mcp/src/tools/knowledge/plan.rs"]
    :summary "Committed wave26-03 in fcd937a798ba (feat(plan): surface router backend readiness). 2 files / 1242 insertions / 4 deletions inside write-scope. mission_plan execute schema gains optional MCP arg router_backend_registry_path; daemon module router_policy_dry_run adds: BackendEntry struct + BackendRegistryInfo enum (Absent | Used | Missing | Unreadable | Malformed) + parse_backend_registry minimal Lisp parser (extracts only :id / :readiness_status / :runtime_allowed / :apply_blockers per (backend ...) entry, tolerating other keys gracefully) + load_backend_registry pure-Rust read + attach_backend_readiness_fields splicing the 6 additive fields with the 6-condition gate inlined. error_block / rejected_block / computed_block now thread BackendRegistryInfo + recommended-backend hint + confidence to keep readiness fields aligned with policy outcome (off/default branch never enters compute_recommendation, preserving wave-15..23 byte-identical baseline). 6-condition apply-eligible gate (mirrors wave26-02 Node logic): policy valid (status=computed) + confidence=high + backend in registry + runtime_allowed=true + readiness_status=runtime-ready + apply_blockers empty. current-default INTENTIONALLY insufficient — explicit runtime-ready opt-in required (documented inline). Failure modes non-fatal: missing/unreadable/malformed/unknown_backend each surface backend_registry_status + backend_warning + force router_apply_eligible=false; dispatch always continues. applied=false stays a literal under EVERY status (re-pinned via applied_remains_false_with_registry covering all 4 status flavours). Off/default mode does NO file I/O even when both router_backend_registry_path AND router_policy_trace_index_path are supplied (re-pinned via router_policy_mode_off_with_registry_and_trace_index_does_no_file_io comparing baseline byte-string to after-attach text). 11 new daemon tests under handlers::knowledge::plan::tests. Acceptance — cargo test -p missiond-daemon handlers::knowledge::plan::tests: 368/368; cargo test -p missiond-daemon: 1659/1659 (1648 baseline + 11 new); cargo test -p missiond-mcp --lib: 17/17 (unchanged from baseline); cargo build --workspace: OK; check-task-contract --all: exit 0 (83 tasks); git diff --check: clean; task-scope-guard --mode staged: 2 staged file(s) OK with 0 must-not-touch matches; verify-task-contract: OK against fcd937a798ba; check-missiond-hooks: severity ok, reason aligned.")

  (claim
    :id wave26-04-claim-001
    :task wave26-04-report-router-readiness-fields-v0
    :agent claudecode
    :seq 10
    :at "2026-04-28T09:50:00+08:00"
    :touched [".missiond/tasks/schema/report-contract-v1.lisp"
              "scripts/check-task-report.mjs"]
    :summary "Claim wave26-04-report-router-readiness-fields-v0: extend report-contract v1 so workers can RECORD backend readiness + apply blockers from the wave26 router readiness loop. Strict additive (5 new optional flat top-level fields): :router_backend_readiness_status (string enum current-default|advisory-only|runtime-ready|unavailable|unknown), :router_backend_runtime_allowed (literal atom bool, NOT strings), :router_apply_eligible (literal atom bool, NOT strings), :router_apply_blockers (vector of non-empty strings), :router_backend_registry_path (repo-relative path string). Reuse wave25-02 helpers validateRouterEnumField / validateRouterRepoRelativePath / validateRouterLiteralBool plus a tight new validateRouterApplyBlockers helper (mirrors validateRouterReasons pattern). Backward-compat non-negotiable: 23 existing fixtures must remain byte-identically green; add 7 new fixtures (legacy-no-readiness pass, all-fields-populated pass with claudecode current-default seed shape + apply_eligible=false, invalid status enum reject, eligible-as-string reject, runtime_allowed-as-string reject, absolute path reject, empty blocker string reject). Fixture count grows 23 → 30. Write scope strictly the 2 named files.")

  (completion
    :id wave26-04-completion-001
    :task wave26-04-report-router-readiness-fields-v0
    :agent claudecode
    :seq 11
    :at "2026-04-28T09:56:00+08:00"
    :touched [".missiond/tasks/schema/report-contract-v1.lisp"
              "scripts/check-task-report.mjs"]
    :summary "Committed wave26-04 in 44ddf9dddd2e (feat(tasks): record router readiness in reports). 2 files / 269 insertions / 5 deletions inside write-scope. Schema (.missiond/tasks/schema/report-contract-v1.lisp) gains: status string updated with wave26-04 stanza; (optional-report-fields ...) extended with 5 new keywords + leading comment block describing additive semantics + cross-wave invariants; (field-contract ...) extended with 5 new declarations documenting purpose / enum members / literal-bool invariant / path discipline; (checker-contract :rejects ...) extended with 5 new rejection strings; non-goal updated to mention router-readiness fields are observational like router-recommendation. Checker (scripts/check-task-report.mjs) gains: ALLOWED_ROUTER_READINESS Set (5 members: current-default | advisory-only | runtime-ready | unavailable | unknown), validateRouterLiteralBoolEither tight wrapper (the wave25-02 validateRouterLiteralBool only accepts a single fixed atom; readiness fields legitimately carry both true and false polarities so a wrapper was added rather than touching the wave25-02 helper signature), validateRouterApplyBlockers helper (mirrors wave25-02 validateRouterReasons pattern: vector of non-empty strings, no property-list entries — wave26-01/02/03 emit plain strings). Wired into validateReport flow as 5 optional checks after the wave25-02 block. Reused wave25-02 helpers for symmetry: validateRouterEnumField (enum) + validateRouterRepoRelativePath (path). Fixture count grew 23 → 30 with 7 new wave26-04 cases: legacy-no-readiness pass; all-5-fields-populated pass mirroring wave26-01 seed (claudecode current-default with apply_eligible=false + 2 explicit blockers naming the 'apply gate requires runtime-ready; current-default is NOT sufficient' pattern); invalid status 'unknown-status' reject; :router_apply_eligible \"false\" string reject; :router_backend_runtime_allowed \"true\" string reject; absolute /abs/path/registry.lisp reject; empty-string blocker reject. Acceptance — check-task-report --dry-fixture: exit 0 (30/30 OK; 23 prior cases byte-identically green); check-task-report --all: exit 0 (52 existing real reports still validate); check-task-contract --all: exit 0 (83 tasks); git diff --check: exit 0; check-missiond-hooks: aligned; task-scope-guard --mode staged: 2 staged file(s) OK with 0 must-not-touch matches; verify-task-contract: OK against 44ddf9dddd2e. Helper-design note: validateRouterLiteralBoolEither is a tight 7-line wrapper (not a refactor of validateRouterLiteralBool) because the wave25-02 helper's expectedAtom positional parameter is exercised by 2 wave25-02 call sites with hard-coded 'true'/'false' that must stay byte-identical. The wave26-04 wrapper adds the both-polarities case without disturbing wave25-02's stricter single-atom contract.")

  (claim
    :id wave26-05-claim-001
    :task wave26-05-renderer-router-readiness-context-v1
    :agent claudecode
    :seq 12
    :at "2026-04-28T20:30:00+08:00"
    :touched ["scripts/render-claudecode-task.mjs"
              ".missiond/tasks/schema/task-contract-v1.lisp"]
    :summary "Claim wave26-05-renderer-router-readiness-context-v1: extend the renderer's wave24-05 + wave25-04 Router Policy (advisory) section to also surface the wave26-01 backend readiness registry, and append a brief Report Contract sub-bullet enumerating the wave26-04 readiness report fields. Add optional kebab-case task-contract field :router-backend-registry-path alongside the wave24-05 :router-policy-path declaration in task-contract-v1.lisp; renderer auto-detects .missiond/router/router-backend-registry-v1.lisp when the field is absent (precedence: explicit field first, then default seed). When the registry resolves, the section appends a read-only `node scripts/check-router-backend-registry.mjs <registry-path>` line; when BOTH the policy AND the registry resolve, the existing wave25-04 recommend-task-backend command gains `--backend-registry <registry-path>` (policy-only path stays byte-identical for backward compat). Section preserves literals 'advisory' + 'dry-run only' verbatim and gains an explicit 'MUST NOT switch backend' phrase. Report Contract section adds a sub-bullet group enumerating the 5 wave26-04 readiness fields (:router_backend_readiness_status enum, :router_backend_runtime_allowed literal bool, :router_apply_eligible literal bool, :router_apply_blockers vector of non-empty strings, :router_backend_registry_path repo-relative path) under the wave25-04 router-recommendation note. Renderer NEVER shells out — commands are text only. Write scope strictly the 2 named files; wave26-* briefs and rendered .md files explicitly out of scope (test renders go to /tmp/).")

  (completion
    :id wave26-05-completion-001
    :task wave26-05-renderer-router-readiness-context-v1
    :agent claudecode
    :seq 13
    :at "2026-04-28T20:51:00+08:00"
    :touched ["scripts/render-claudecode-task.mjs"
              ".missiond/tasks/schema/task-contract-v1.lisp"]
    :summary "Committed wave26-05 in 43df6230bef6 (feat(tasks): render router readiness context). 2 files / 82 insertions / 11 deletions inside write-scope. Renderer changes: (1) loadSingleTask reads :router-backend-registry-path via keywordPropText into routerBackendRegistryPath; (2) resolveRouterBackendRegistryPath helper mirrors resolveRouterPolicyPath precedence (explicit field first, then default .missiond/router/router-backend-registry-v1.lisp seed), returns null when neither resolves; (3) renderTask threads routerBackendRegistryPath into renderRouterPolicy as the new 5th positional arg; (4) renderRouterPolicy extended to print the 'Backend readiness registry: <path>' header line when the registry resolves, the explicit 'MUST NOT switch backend' bullet always (independent of registry resolution), per-source 'Policy source' / 'Registry source' bullets, an appended `node scripts/check-router-backend-registry.mjs <registry-path>` line in the read-only checker fenced block when the registry resolves, and (when BOTH policy AND registry resolve) a recommend-task-backend command line extended with `--backend-registry <registry-path>` — the policy-only command stays byte-identical when the registry does not resolve (backward compat); (5) renderReportContract appends a sub-bullet group enumerating the 5 wave26-04 readiness fields with type/enum hints + cross-wave invariants restated (current-default alone NEVER sufficient — explicit runtime-ready opt-in required upstream). Schema changes (.missiond/tasks/schema/task-contract-v1.lisp): :status string updated with wave26-05 stanza; new optional :router-backend-registry-path field-contract entry next to :router-policy-path documenting auto-detect precedence + section integration + 'MUST NOT switch backend' invariant + renderer never-shells-out guarantee; renderer-contract :machine-context-rendered + :backward-compatibility extended with router-backend-registry context + report-contract router-readiness sub-bullet entries. Section flow preserved byte-identical (Machine Contract → Goal → Ownership → Must Not Touch → Requirements → Acceptance Commands → Shared Memory → Report Contract → Session Trace → Router Policy → Commit → Report). 'advisory' + 'dry-run only' literals preserved verbatim. Renderer NEVER shells out — zero new fork/exec/spawn/child_process calls (no `child_process` import, no `spawn`/`exec`/`execSync` calls). Acceptance — node scripts/render-claudecode-task.mjs --stdout .missiond/tasks/wave26/wave26-02-router-recommendation-readiness-v1.lisp > /tmp/wave26-router-readiness.md: exit 0; rg 'Router Policy|router-backend-registry|--backend-registry|advisory|dry-run only' /tmp/wave26-router-readiness.md: all 5 required patterns present; node scripts/check-task-contract.mjs --all: exit 0 (83 tasks); node scripts/check-task-report.mjs --dry-fixture: exit 0 (30/30 — wave26-04 fixtures byte-identically green); git diff --check -- scripts/render-claudecode-task.mjs .missiond/tasks/schema/task-contract-v1.lisp: clean; task-scope-guard --mode staged: 2 staged file(s) OK with 0 must-not-touch matches; verify-task-contract: OK against 43df6230bef6; check-missiond-hooks: severity ok, reason aligned.")

  (claim
    :id wave26-06-claim-001
    :task wave26-06-router-readiness-smoke-v1
    :agent claudecode
    :seq 14
    :at "2026-04-28T21:00:00+08:00"
    :touched ["scripts/recommend-task-backend.mjs"
              "scripts/evaluate-router-policy-corpus.mjs"
              "scripts/check-task-report.mjs"
              "scripts/render-claudecode-task.mjs"
              "crates/missiond-daemon/src/handlers/knowledge/plan.rs"]
    :summary "Claim wave26-06-router-readiness-smoke-v1: add cross-layer smoke coverage proving wave26 backend readiness annotations stay advisory and CANNOT apply runtime replacement. Layer A1 (Node recommend dry-fixture) adds wave26-06 readiness-eligible-smoke + readiness-current-default-blocked-smoke + readiness-static-audit cases (recommend baseline 17 -> 20). Layer A2 (Node evaluator dry-fixture) adds wave26-06-corpus-aggregates-readiness-smoke (evaluator baseline 11 -> 12). Layer B (Rust daemon) adds router_policy_dry_run_smoke_pins_wave26_invariants + router_policy_cli_rust_parity_for_readiness tests pinning all 9 cross-wave invariants under the wave26-03 code path with self-audit. Layer C (report checker) adds wave26-06-readiness-all-fields-claudecode-smoke pass fixture (report baseline 30 -> 31). Layer D (renderer) adds wave26-06-renderer-readiness-literals smoke fixture inline that renders a synthetic task and asserts 'advisory' + 'dry-run only' + 'router-backend-registry' + '--backend-registry' + 'MUST NOT switch backend' literals all present. Static-audit forbidden patterns assembled from string parts to avoid self-trip (wave24-06 / wave25-05 lesson). Pure-in-process: zero spawn/child_process/git/network in the smoke path. Write scope strictly the 5 named files.")

  (completion
    :id wave26-06-completion-001
    :task wave26-06-router-readiness-smoke-v1
    :agent claudecode
    :seq 15
    :at "2026-04-28T21:31:00+08:00"
    :touched ["scripts/recommend-task-backend.mjs"
              "scripts/evaluate-router-policy-corpus.mjs"
              "scripts/check-task-report.mjs"
              "scripts/render-claudecode-task.mjs"
              "crates/missiond-daemon/src/handlers/knowledge/plan.rs"]
    :summary "Committed wave26-06 in 4bfa710e4489 (test(router): smoke backend readiness loop). 5 files / 937 insertions inside write-scope. All 9 cross-wave invariants pinned across 5 layers. LAYER A1 (recommend-task-backend.mjs +3 fixtures, baseline 17 -> 20): wave26-06-readiness-eligible-smoke (synthetic runtime-ready registry => apply_eligible=true with literal-bool + dry_run_only=true + applied-not-on-recommend invariants); wave26-06-readiness-current-default-blocked-smoke (seed-shape current-default registry => apply_eligible=false with canonical 'current-default is NOT sufficient' blocker text); wave26-06-readiness-static-audit (scans all 4 router scripts for forbidden patterns assembled from string parts). LAYER A2 (evaluate-router-policy-corpus.mjs +1 fixture, baseline 11 -> 12): wave26-06-corpus-aggregates-readiness-smoke pins apply_eligible_count=0 for seed-shape registry under high-confidence trace (current-default INSUFFICIENT). LAYER B (plan.rs +2 daemon tests under handlers::knowledge::plan::tests, baseline 1659 -> 1661): router_policy_dry_run_smoke_pins_wave26_invariants exercises BOTH polarities back-to-back (seed-shape => apply_eligible=Bool(false), synthetic runtime-ready => apply_eligible=Bool(true)) plus Off+both-router-args byte-shape invariant 7 plus dispatch-byte-identical re-pin plus self-audit on plan.rs; router_policy_cli_rust_parity_for_readiness pins CLI/Rust agreement on backend_readiness_status='current-default' + backend_runtime_allowed=true + router_apply_eligible=false for the seed-shape registry + (8,8)-event trace + docs->claudecode rule (mirrors Node Layer A1 wave26-06-readiness-current-default-blocked-smoke fixture exactly so a regression on either side fails BOTH sides). LAYER C (check-task-report.mjs +1 positive fixture, baseline 30 -> 31): wave26-06-readiness-all-fields-claudecode-smoke validates the canonical wave26-01 seed-shape report (claudecode current-default + apply_eligible=false + canonical blocker text + repo-relative registry path). LAYER D (render-claudecode-task.mjs +--dry-fixture mode + 2 fixtures): wave26-06-renderer-readiness-literals renders a synthetic task with both router fields and asserts 'advisory' + 'dry-run only' + 'router-backend-registry' + '--backend-registry' + 'MUST NOT switch backend' literals all present (plus both router file paths verbatim and the recommend command WITH --backend-registry); wave26-06-renderer-static-audit scans the renderer module for forbidden patterns. Helper added: top-level `import os from 'node:os'` (only fs+path were imported before — os was needed for the dry-fixture's mkdtempSync). Acceptance — recommend dry-fixture: 20/20 OK; evaluate dry-fixture: 12/12 OK; check-task-report dry-fixture: 31/31 OK (52 real reports also still validate); renderer dry-fixture: 2/2 OK; render of wave26-02.lisp -> /tmp/wave26-router-smoke.md: exit 0; rg literal grep: 20 hits across 5 patterns; cargo test plan::tests: 370/370 OK; cargo test daemon: 1661/1661 OK; cargo build --workspace: OK; check-task-contract --all: 83 OK; git diff --check: clean; task-scope-guard --mode staged: 5 staged file(s) OK with 0 must-not-touch matches; verify-task-contract: OK against 4bfa710e4489; check-missiond-hooks: severity ok, reason aligned. Audit (cross-wave invariant 9): zero new spawn/child_process/exec/fork/openai/anthropic/fetch/https calls in any modified file — every static-audit fixture asserts this on its target file with patterns assembled from string parts to avoid self-trip (wave24-06 / wave25-01 / wave25-05 lesson)."))

