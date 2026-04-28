;; Wave 27 / Task 05 — Renderer router dispatch descriptor context v0.
;; Schema: missiond.report-contract.v1
;; Source: .missiond/tasks/wave27/wave27-05-renderer-router-dispatch-descriptor-context-v0.lisp

(report wave27-05-renderer-router-dispatch-descriptor-context-v0
  :schema "missiond.report-contract.v1"
  :task_id "wave27-05-renderer-router-dispatch-descriptor-context-v0"
  :status done
  :commit_hash "17cb401f10746f659389de159ba7381c2fe560da"
  :files_changed
    ["scripts/render-claudecode-task.mjs"
     ".missiond/tasks/schema/task-contract-v1.lisp"]

  :acceptance_results
    [(:command "node scripts/render-claudecode-task.mjs --dry-fixture"
      :exit_code 0
      :ok true
      :notes "render-claudecode-task fixtures OK (4 cases, 4 categories). Baseline 2 wave26-06 fixtures (wave26-06-renderer-readiness-literals + wave26-06-renderer-static-audit) stay byte-identical green; 2 new wave27-05 fixtures pass: wave27-05-renderer-dispatch-descriptor-literals (asserts advisory + dry-run only + no execution + MUST NOT switch backend + build-router-dispatch-descriptor literals AND the pipe-to-check-router-dispatch-descriptor.mjs --stdin form AND the regression that pipe form must NOT contain --json AND all 6 wave27-04 report fields enumerated AND wave26-05 --backend-registry preserved on recommend-task-backend); wave27-05-renderer-static-audit (scans renderer source for forbidden child_process / spawn / spawnSync / execSync / execFile / fork / openai / anthropic / chat.completion / fetch / https.get|request|post / net.createConnection / simpleGit — pattern table assembled from string parts so audit does not self-trip).")
     (:command "node scripts/render-claudecode-task.mjs --stdout .missiond/tasks/wave27/wave27-02-router-dispatch-descriptor-cli-v0.lisp > /tmp/wave27-router-dispatch-descriptor.md"
      :exit_code 0
      :ok true
      :notes "Rendered 184 lines. Section flow preserved: Machine Contract -> Goal -> Ownership -> Must Not Touch -> Requirements -> Acceptance Commands -> Shared Memory -> Report Contract (+ wave27-04 sub-bullet group with all 6 dispatch-descriptor fields) -> Session Trace -> Router Policy (advisory) (+ wave27-05 dispatch-descriptor sub-section with both Lisp + pipe commands AFTER wave26-05 recommend-task-backend block) -> Commit -> Report.")
     (:command "rg 'Router Policy|dispatch descriptor|no execution|MUST NOT switch backend|build-router-dispatch-descriptor' /tmp/wave27-router-dispatch-descriptor.md"
      :exit_code 0
      :ok true
      :notes "All 5 patterns present. 'Router Policy' header (1 hit) + 'dispatch descriptor' phrase (3 hits including the new sub-section preamble + wave27-04 report-fields header) + 'no execution' literal (1 hit in the dispatch-descriptor sub-section preamble) + 'MUST NOT switch backend' literal (3 hits across wave26-05 + wave25-04 + wave27-05 sub-sections) + 'build-router-dispatch-descriptor' name (3 hits including the 2 rendered command lines + 1 in the wave27-04 report-fields sub-bullet header).")
     (:command "node scripts/check-task-contract.mjs --all"
      :exit_code 0
      :ok true
      :notes "task-contract check OK (92 tasks). Schema status string update + 2 new renderer-contract entries (machine-context + backward-compatibility) parse cleanly; no regressions across wave22..wave27 task contracts (also 92 baseline)."
     )
     (:command "git diff --check -- scripts/render-claudecode-task.mjs .missiond/tasks/schema/task-contract-v1.lisp"
      :exit_code 0
      :ok true
      :notes "no whitespace errors on either staged path; trailing-newline / tab-stop hygiene clean.")
     (:command "node scripts/check-missiond-hooks.mjs --json"
      :exit_code 0
      :ok true
      :notes "preflight OK; core.hooksPath aligned to .githooks; .githooks/pre-commit exists and is executable; no install required.")
     (:command "node scripts/task-scope-guard.mjs --task .missiond/tasks/wave27/wave27-05-renderer-router-dispatch-descriptor-context-v0.lisp --mode staged"
      :exit_code 0
      :ok true
      :notes "task-scope-guard staged OK: wave27-05-renderer-router-dispatch-descriptor-context-v0 (2 staged file(s)) — both staged paths inside :write-scope (scripts/render-claudecode-task.mjs + .missiond/tasks/schema/task-contract-v1.lisp); zero matches against :must-not-touch (crates/** .missiond/v2/** .missiond/router/** .missiond/tasks/wave27/wave27-*.lisp .missiond/claudecode/** scripts/check-router-dispatch-descriptor.mjs scripts/build-router-dispatch-descriptor.mjs scripts/check-task-report.mjs scripts/recommend-task-backend.mjs scripts/evaluate-router-policy-corpus.mjs).")
     (:command "node scripts/build-router-dispatch-descriptor.mjs --task .missiond/tasks/wave27/wave27-02-router-dispatch-descriptor-cli-v0.lisp --policy .missiond/router/router-policy-v1.lisp --backend-registry .missiond/router/router-backend-registry-v1.lisp | node scripts/check-router-dispatch-descriptor.mjs --stdin"
      :exit_code 0
      :ok true
      :notes "Live smoke of the rendered pipe form: descriptor build + check exit 0 (1 descriptor OK). Confirms the rendered command shape is executable end-to-end and that dropping --json on the pipe form is correct (wave27-01 checker only parses Lisp on stdin per wave27-02 finding).")
     (:command "node scripts/verify-task-contract.mjs .missiond/tasks/wave27/wave27-05-renderer-router-dispatch-descriptor-context-v0.lisp"
      :exit_code 0
      :ok true
      :notes "task-contract verify OK: wave27-05-renderer-router-dispatch-descriptor-context-v0 against 17cb401f1074 — commit hash exists; commit message matches `feat(tasks): render router dispatch descriptor context` per contract; changed_files ⊆ write-scope (write-scope-only); changed_files ∩ must-not-touch = ∅; acceptance commands present in contract.")]

  :scope_deviations []

  :trace_refs [wave27-trace-05-start-001 wave27-trace-05-commit-001 wave27-trace-05-complete-001]

  :major_decisions
    [(:decision "Do NOT add a new optional task-contract field (no :router-dispatch-descriptor-path)."
      :rationale "The dispatch descriptor is an EPHEMERAL artifact: the wave27-02 CLI builds it on demand from existing inputs (task contract + router policy + backend registry). A static :router-dispatch-descriptor-path field on every task contract would create stale-by-design state — the descriptor needs to be regenerated whenever any of its three inputs changes, so persisting a path adds maintenance burden with no readability gain. Instead the renderer surfaces the build command parameterized by the SAME paths it already auto-detects (router-policy + router-backend-registry). Decision documented inline in the schema renderer-contract status string + new :machine-context-rendered entry + new :backward-compatibility entry.")
     (:decision "Render the pipe-to-checker form WITHOUT --json (drop --json from the second command line)."
      :rationale "The wave27-01 check-router-dispatch-descriptor.mjs --stdin code path only parses Lisp, not JSON (wave27-02 commit 14fdf5a investigation finding). Rendering `... --json | check --stdin` would print rendered text that errors at runtime and erode trust in the brief. The default Lisp output of build-router-dispatch-descriptor.mjs is already pipe-friendly, so the pipe form is `build ... | check --stdin`. The dispatch-descriptor literals fixture asserts BOTH presence of the pipe form AND absence of --json on the pipe form so a future regression that 'helpfully' adds --json is caught immediately.")
     (:decision "Place the dispatch-descriptor sub-section AFTER the wave26-05 recommend-task-backend block, INSIDE the existing Router Policy (advisory) section."
      :rationale "Brief explicitly says 'Extend the existing Router Policy (advisory) section (don't add a new section; keep section flow intact)'. Sub-section ordering: wave24-05 check-router-policy → wave26-05 check-router-backend-registry → wave25-04 + wave26-05 recommend-task-backend → wave27-05 build-router-dispatch-descriptor. This matches the data-flow order: read policy → read registry → read recommendation → derive descriptor. wave26-05 surface stays byte-identical when only the policy resolves (no new sub-section appears).")
     (:decision "Render the dispatch-descriptor sub-section ONLY when BOTH policy AND registry resolve (gate on routerBackendRegistryPath being truthy)."
      :rationale "The wave27-02 CLI requires --task + --policy + --backend-registry to emit a descriptor (registry is REQUIRED for descriptor mode per wave27-02 commit 14fdf5a). If the registry is missing, the build command would fail at runtime — rendering it would invite confusion. The conditional matches the wave26-05 pattern of only appending the registry context when the registry resolves.")
     (:decision "Add a new wave27-05 static-audit fixture rather than extending the wave26-06 audit in place."
      :rationale "Brief explicitly asks for 'wave27-05-renderer-static-audit' as a separate fixture name. The wave26-06 audit stays byte-identical (proves no regression in the original pattern table). The wave27-05 audit adds 3 EXTRA forbidden patterns (execFile, net.createConnection, simpleGit) that the wave26-06 audit does not cover, providing defense-in-depth on the renderer's no-shell-out invariant.")]

  :time_sinks
    [(:label "Reading existing renderRouterPolicy + renderReportContract + runFixtures end-to-end before editing"
      :notes "Confirmed the wave24-05 / wave25-04 / wave26-05 surface contracts (advisory / dry-run only / MUST NOT switch backend literals + recommend-task-backend command parameterization + Report Contract sub-bullet groups) so wave27-05 additions slot in without regression. Also re-read wave27-02 build-router-dispatch-descriptor.mjs CLI flags + wave27-01 checker --stdin behavior to confirm pipe form + JSON-vs-Lisp finding.")
     (:label "Drafting the dispatch-descriptor sub-section preamble with all 4 required literals in one sentence"
      :notes "Phrase 'advisory, dry-run only, and no execution' carries 3 of the 4 literals; 'MUST NOT switch backend' is repeated in the same preamble line. Single-line preamble keeps the section visually compact and matches the wave25-04 + wave26-05 preamble style.")
     (:label "Verifying live pipe smoke (build | check --stdin) against seed policy + seed registry"
      :notes "Ran the rendered command verbatim to confirm exit 0 and 1 descriptor produced. Confirms the rendered text is executable end-to-end so workers can copy-paste safely.")]

  :unexpected_work []

  :recommended_backend "claudecode"
  :router_confidence "high"
  :router_policy_path ".missiond/router/router-policy-v1.lisp"
  :router_dry_run_only true
  :router_applied false
  :router_reasons
    ["Workstation surface (Node.js renderer Edit + Lisp schema Edit, additive only, no Rust / SQL / cargo) is the canonical claudecode beat — matches r-fresh-code-alignment-to-claudecode in router-policy-v1."
     "Strict additive backward-compat constraint required all 2 baseline wave26-06 fixtures + the wave24-05 / wave25-04 / wave26-05 rendered Router Policy section content to stay byte-identical green; ClaudeCode is the established default for low-risk renderer extensions."
     "Router output is recorded for telemetry only; runtime dispatch unchanged (claudecode is the live default and remained the live default for this task)."]
  :router_trace_index_path ".missiond/router/trace-index-v1.lisp"

  :router_backend_readiness_status "current-default"
  :router_backend_runtime_allowed true
  :router_apply_eligible false
  :router_apply_blockers
    ["backend claudecode readiness_status=current-default (apply gate requires runtime-ready; current-default is NOT sufficient)"
     "explicit runtime-ready opt-in required upstream before live promotion"]
  :router_backend_registry_path ".missiond/router/router-backend-registry-v1.lisp"

  :router_dispatch_descriptor_path ".missiond/router/dispatch-descriptors/wave27-05-renderer-router-dispatch-descriptor-context-v0.lisp"
  :router_dispatch_descriptor_status "absent"
  :router_dispatch_backend "claudecode"
  :router_dispatch_eligible false
  :router_dispatch_no_execution true
  :router_dispatch_blockers
    ["wave27-02 builder has not yet emitted a persisted descriptor for this task (descriptor_status=absent records the handoff fact without claiming runtime backend execution)"
     "descriptor recording NEVER asserts a runtime backend swap happened (cross-wave invariant — :router_dispatch_no_execution locked literal true)"
     "apply gate requires runtime-ready; current-default is NOT sufficient"]

  :notes
    "wave27-05 ships:
     - scripts/render-claudecode-task.mjs: renderRouterPolicy() extended with a new wave27-05 dispatch-descriptor sub-section appended AFTER the wave26-05 recommend-task-backend block, gated on routerBackendRegistryPath resolving (matches the wave27-02 CLI's hard requirement on --backend-registry). Sub-section preamble carries the literals 'advisory', 'dry-run only', 'no execution', and 'MUST NOT switch backend' verbatim. TWO command lines emitted in a fenced block: (1) `node scripts/build-router-dispatch-descriptor.mjs --task <task> --policy <policy> --backend-registry <registry>` (default Lisp output, pipe-friendly), (2) the same command piped into `node scripts/check-router-dispatch-descriptor.mjs --stdin` (no --json on the pipe form per wave27-02 finding). renderReportContract() extended with a new sub-bullet group enumerating all 6 wave27-04 dispatch-descriptor fields with MAY-language (:router_dispatch_descriptor_path repo-relative path, :router_dispatch_descriptor_status enum eligible|current-default|advisory-only|registry-missing|unavailable|unknown, :router_dispatch_backend enum 5-value backend, :router_dispatch_eligible literal bool, :router_dispatch_no_execution literal `true` only — cross-wave invariant — false AND quoted-string both rejected by the checker, :router_dispatch_blockers vector of non-empty strings). Function-level docstring on renderRouterPolicy extended with a wave27-05 paragraph documenting the new behaviour. Two new --dry-fixture cases added (wave27-05-renderer-dispatch-descriptor-literals + wave27-05-renderer-static-audit); fixture count 2 -> 4. usage docstring updated to reference 'no execution' and 'build-router-dispatch-descriptor' literals.
     - .missiond/tasks/schema/task-contract-v1.lisp: schema renderer-contract :status string extended with a wave27-05 paragraph documenting the new surface and the explicit decision NOT to add a new optional task-contract field. New :machine-context-rendered entry 'router-dispatch-descriptor commands (wave27-05)' enumerates the new dispatch-descriptor sub-section behaviour, the 2 command lines emitted, the gating on policy + registry resolution, the 'no execution' literal addition, and the no-new-field rationale. New :machine-context-rendered entry 'report-contract router-dispatch-descriptor note (wave27-05)' enumerates the 6 wave27-04 fields and their MAY-language. New :backward-compatibility entries mirror these additions.

     Decision: NO new optional task-contract field added (no :router-dispatch-descriptor-path). Rationale: the dispatch descriptor is an ephemeral artifact (wave27-02 CLI builds it on demand from existing inputs — task contract + router policy + backend registry). A static :router-dispatch-descriptor-path field would create stale-by-design state. The renderer surfaces the build command parameterized by the SAME paths it already auto-detects (router-policy + router-backend-registry). Documented inline in renderer-contract status string + 2 new :machine-context-rendered entries + 2 new :backward-compatibility entries.

     Verbatim sample of the 2 new rendered command lines (parameterized for wave27-02 brief):
       node scripts/build-router-dispatch-descriptor.mjs --task .missiond/tasks/wave27/wave27-02-router-dispatch-descriptor-cli-v0.lisp --policy .missiond/router/router-policy-v1.lisp --backend-registry .missiond/router/router-backend-registry-v1.lisp
       node scripts/build-router-dispatch-descriptor.mjs --task .missiond/tasks/wave27/wave27-02-router-dispatch-descriptor-cli-v0.lisp --policy .missiond/router/router-policy-v1.lisp --backend-registry .missiond/router/router-backend-registry-v1.lisp | node scripts/check-router-dispatch-descriptor.mjs --stdin

     Section flow placement (Router Policy section, top to bottom): wave24-05 preamble (advisory + dry-run only) -> wave26-05 readiness preamble (MUST NOT switch backend) -> Policy/Registry source bullets -> wave24-05 + wave26-05 inspect-checkers fenced block (check-router-policy + check-router-backend-registry) -> wave25-04 + wave26-05 recommend-task-backend preamble + fenced block (with --backend-registry when registry resolves) -> [NEW wave27-05] dispatch-descriptor preamble (advisory + dry-run only + no execution + MUST NOT switch backend) + fenced block (build-router-dispatch-descriptor.mjs + the same piped to check-router-dispatch-descriptor.mjs --stdin).

     'no execution' phrase appears in the wave27-05 dispatch-descriptor sub-section preamble: '... the commands below are rendered text only and stay **advisory**, **dry-run only**, and **no execution**; the descriptor never changes dispatch and you MUST NOT switch backend on the strength of its output:'. wave26-05 'MUST NOT switch backend' literal preserved verbatim (3 hits in rendered brief: wave24-05/wave26-05 preamble + wave25-04/wave26-05 recommend-task-backend preamble + new wave27-05 dispatch-descriptor preamble). wave24-05 'advisory' / 'dry-run only' literals preserved verbatim and now appear in 3 sub-section preambles each.

     6 wave27-04 dispatch-descriptor report fields enumerated in the Report Contract sub-bullet group: :router_dispatch_descriptor_path / :router_dispatch_descriptor_status / :router_dispatch_backend / :router_dispatch_eligible / :router_dispatch_no_execution / :router_dispatch_blockers. Each carries the type/enum hint and (for the 2 literal-bool fields) the locked-atom invariant. The `:router_dispatch_no_execution` bullet explicitly says 'literal `true` ONLY (cross-wave invariant; literal `false` AND any quoted-string form are rejected by the checker)'.

     --dry-fixture count: baseline 2 (wave26-06-renderer-readiness-literals + wave26-06-renderer-static-audit) + 2 new wave27-05 (wave27-05-renderer-dispatch-descriptor-literals + wave27-05-renderer-static-audit) = 4 total across 4 categories.

     Pre-commit pipeline: render --dry-fixture (exit=0, 4/4) -> render --stdout wave27-02 -> rg 5 patterns (exit=0) -> check-task-contract --all (exit=0, 92 tasks) -> git diff --check (exit=0) -> check-missiond-hooks --json (preflight aligned) -> live pipe smoke build|check --stdin (exit=0, 1 descriptor OK) -> git add (2 paths) -> task-scope-guard --mode staged (OK, 2 staged) -> MISSIOND_TASK_CONTRACT=... git commit -m \"feat(tasks): render router dispatch descriptor context\" (commit 17cb401f1074) -> verify-task-contract (OK against 17cb401f1074). All append-only ledger updates: shared-memory wave27-05-claim-001 (seq 12) before edits + wave27-05-completion-001 (seq 13) after verify; session-trace wave27-trace-05-start-001 (seq 18) + wave27-trace-05-commit-001 (seq 19, with commit_hash) + wave27-trace-05-complete-001 (seq 20). Both ledgers re-validated after each append.

     Constraints honored: NO Rust / SQL / Cargo edits. Did not touch crates/**, .missiond/v2/**, .missiond/router/**, any wave27-*.lisp other than session-trace + shared-memory (both are session-trace-writable / claim-allowed and explicitly NOT in :must-not-touch), .missiond/claudecode/**, scripts/check-router-dispatch-descriptor.mjs, scripts/build-router-dispatch-descriptor.mjs, scripts/check-task-report.mjs, scripts/recommend-task-backend.mjs, scripts/evaluate-router-policy-corpus.mjs. Did not Re-render any wave27-* brief. Did not git add . / git push / --no-verify / --amend / --force.")
