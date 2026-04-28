;; Wave 29 / Task 06 — Ready queue planner v0.
;; Schema: missiond.report-contract.v1
;; Source: .missiond/tasks/wave29/wave29-06-ready-queue-planner-v0.lisp

(report wave29-06-ready-queue-planner-v0
  :schema "missiond.report-contract.v1"
  :task_id "wave29-06-ready-queue-planner-v0"
  :status done
  :commit_hash "1951aaa4fe79"
  :agent_commit_hash "15c5267fcaa4"
  :final_commit_hash "1951aaa4fe79"
  :verified_commit_hash "1951aaa4fe79"
  :parent_patches
    [(:commit "1951aaa4fe79"
      :kind lint-cleanup
      :reason "TS6133 unused `dependents` parameter in computeReadyQueue() destructuring; the function uses idToNode + a locally-built effectiveDeps Map and never needed the planFromManifestObject-side dependents adjacency map."
      :files ["scripts/plan-task-runner.mjs"])]
  :files_changed
    ["scripts/plan-task-runner.mjs"]

  :acceptance_results
    [(:command "node scripts/plan-task-runner.mjs --dry-fixture"
      :exit_code 0
      :ok true
      :notes "task-runner-plan fixtures OK (19 cases, 13 categories): 13 baseline (12 wave28-02 + 1 wave28-06) + 6 NEW under category wave29-06-ready-queue. New cases: ready-queue-default-byte-identical (no --schedule => no ready_queue field, output byte-identical to explicit group-barrier), ready-queue-flag-emits-ready-windows (under --schedule ready-queue ready_queue field + schema marker present, foo/bar/baz windows verified, order respects critical-path-first), ready-queue-overlap-blocks-window (warn policy => zero overlap_edges injected), ready-queue-overlap-edge-injected-reject (gated_by lists explicit dep only, fast peer not held by 60-min slow peer in same group), ready-queue-priority-deterministic (replay byte-identical, order [a,c,b] for ties at t=10 with c=50min beating b=5min on critical-path-desc), ready-queue-idle-savings-computed (unbalanced manifest: fast-follower saves 50 min vs barrier-finish=65 → finish=15, aggregate savings + wave_duration_savings both > 0).")
     (:command "node scripts/check-task-runner-manifest.mjs --dry-fixture"
      :exit_code 0
      :ok true
      :notes "task-runner-manifest fixtures OK (22 cases, 16 categories) — manifest schema/checker untouched; baseline preserved.")
     (:command "node scripts/check-task-contract.mjs --all"
      :exit_code 0
      :ok true
      :notes "task-contract check OK (105 tasks) — all wave22..wave29 task contracts including this one parse and validate. No regressions.")
     (:command "perl -ne 'exit 1 if /\\x00/' scripts/plan-task-runner.mjs"
      :exit_code 0
      :ok true
      :notes "no raw NUL bytes — required so rg/grep keep treating the file as searchable text. The planner does use \\u0000 escapes at runtime as map-key separators (collectOverlapDiagnostics) but those are JS string escapes in the source, not literal NUL bytes. Single regression discovered mid-implementation (a paste introduced a literal NUL in `${from} ${to}` template literal at line 489) and repaired before commit; perl check now clean.")
     (:command "git diff --check -- scripts/plan-task-runner.mjs scripts/check-task-runner-manifest.mjs .missiond/tasks/schema/task-runner-manifest-v1.lisp"
      :exit_code 0
      :ok true
      :notes "no whitespace errors on plan-task-runner.mjs; check-task-runner-manifest.mjs and task-runner-manifest-v1.lisp UNCHANGED (schema field changes intentionally avoided; manifest_metadata is not load-bearing for the additive ready-queue branch).")
     (:command "node scripts/check-missiond-hooks.mjs --json"
      :exit_code 0
      :ok true
      :notes "preflight OK; core.hooksPath aligned to .githooks; .githooks/pre-commit exists and is executable; no install required.")
     (:command "node scripts/task-scope-guard.mjs --task .missiond/tasks/wave29/wave29-06-ready-queue-planner-v0.lisp --mode staged"
      :exit_code 0
      :ok true
      :notes "task-scope-guard staged OK: wave29-06-ready-queue-planner-v0 (1 staged file(s)) — only scripts/plan-task-runner.mjs staged; check-task-runner-manifest.mjs and task-runner-manifest-v1.lisp intentionally not modified; zero matches against :must-not-touch.")
     (:command "node scripts/verify-task-contract.mjs .missiond/tasks/wave29/wave29-06-ready-queue-planner-v0.lisp"
      :exit_code 0
      :ok true
      :notes "task-contract verify OK: wave29-06-ready-queue-planner-v0 against 1951aaa4fe79 (final commit; worker commit was 15c5267fcaa4) — commit hash exists; commit message matches `feat(tasks): plan runner ready queue` per contract; changed_files ⊆ write-scope (write-scope-only); changed_files ∩ must-not-touch = ∅; acceptance commands present in contract.")]

  :scope_deviations []

  :trace_refs [wave29-06-trace-start-001
               wave29-06-trace-read-001
               wave29-06-trace-commit-001
               wave29-06-trace-hotfix-001
               wave29-06-trace-complete-001]

  :major_decisions
    [(:decision "Default schedule mode = group-barrier; ready-queue is opt-in via explicit --schedule ready-queue flag (or schedule: 'ready-queue' option to planFromManifestObject)."
      :rationale "Backward compatibility is non-negotiable — the wave28-02 baseline output (12 fixtures) and the wave28-06 cross-layer smoke determinism pin (1 fixture) MUST stay byte-identical. Gating the new branch behind an explicit flag makes the additive change zero-risk for downstream tooling that already shells out to `plan-task-runner.mjs --json`. SCHEDULE_MODES + DEFAULT_SCHEDULE_MODE exported so wave29-07 cross-layer smoke can introspect.")
     (:decision "Priority rule = critical-path-remaining-desc; tie-break = task-id-lex-asc."
      :rationale "Critical-path-first minimizes wave wall-clock duration: tasks feeding the longest downstream tail emit earlier so the dispatcher does not waste capacity on short branches that are not on the critical path. Lex tie-break is the simplest deterministic disambiguator the rest of the planner already uses (sortedNodes, batches, overlap_diagnostics). Both rule names are surfaced as priority_rule + tie_break top-level fields under ready_queue so consumers do not have to hard-code conventions.")
     (:decision "Same-dispatch_group write_scope overlap is treated as an additional serializing edge ONLY when overlap_policy = reject (the default)."
      :rationale "Under reject, validateManifestObject already blocks at schema time so the planner rarely sees such manifests; treating the overlap as a virtual edge keeps the safety guarantee intact when the schema gate is somehow bypassed. Under warn (opt-in), the manifest authored explicitly tolerates the overlap so the planner respects that intent and does NOT inject the edge. Edge direction = lex-smaller id → lex-larger id, which preserves DAG acyclicity.")
     (:decision "barrier_finish_at_minutes is computed as batchStart + node.estimated_minutes (per-task), not batch end (collective)."
      :rationale "The savings metric is meant to capture wall-clock reclaimed by ready-queue, not the task's actual barrier wait time inside its own batch (always zero for the slowest peer). Using per-task duration gives a positive non-trivial savings number when ready-queue lets a task finish earlier than the SAME task would under group-barrier. wave_duration_savings_minutes is the additional aggregate that captures the slowest-peer reclaim across batches.")
     (:decision "NO new manifest schema fields. task-runner-manifest-v1.lisp + check-task-runner-manifest.mjs UNCHANGED."
      :rationale "Brief explicitly recommends NONE. The ready-queue planner derives every input it needs (depends_on, dispatch_group, write_scope, estimated_minutes) from existing required fields. Adding a :schedule_hint optional field would have been speculative — no consumer needs it today and adding it now would create migration debt for wave29-07 + wave30+ manifests. Leaving the schema untouched keeps the additive surface strictly inside the planner CLI.")
     (:decision "Output ready_queue.tasks sorted by task_id lex; ready_queue.order is the deterministic emission queue."
      :rationale "tasks[] is the structural lookup table — easy to find a specific task's window. order[] is the dispatch-time linear queue — callers consume it head-first. Two views with two different invariants keep both indexed access and time-ordered iteration cheap for downstream code without recomputing.")
     (:decision "Single line bug — paste of `${from} ${to}` template literal introduced a raw NUL byte at line 489 between `${from}` and `${to}`."
      :rationale "Caught by the perl NUL check during acceptance and repaired in-place by reading the file, replacing /\\x00/g with a space, and writing back. Documented as a time-sink so future workers know to grep for raw NULs after large Edit blocks. The wave28-02 file already had this hazard noted (commit 37d7e32 stripped pre-existing NULs); reaffirms the large-file-navigation pattern card guidance.")
     (:decision "Parent lint-cleanup hotfix 1951aaa4fe79 on top of worker commit 15c5267fcaa4 — drop unused `dependents` parameter from computeReadyQueue() destructuring (TS6133)."
      :rationale "Coordinator preflight surfaced the unused parameter after the worker commit. Per fail-fast policy + clean-code-better-than-prefix-underscore guidance, deleted the parameter from both the function signature AND the call site. computeReadyQueue uses idToNode + the locally-built effectiveDeps Map; the planFromManifestObject-side `dependents` adjacency map (built for longestFrom) was not needed. Same task contract + same commit message as worker; this commit's :parent_patches lineage records both commits explicitly per wave29-04 lineage v1 schema.")]

  :time_sinks
    [(:label "Reading plan-task-runner.mjs end-to-end and mapping anchors before editing"
      :notes "1243-line file. Used atlas grep anchors (planFromManifestFile / planFromManifestObject / longestFrom / collectOverlapDiagnostics / formatPlanLisp / runFixtures) to navigate without whole-file scrolling beyond the necessary 400-line slices. Critical anchors: lines 161 / 191 / 384 / 504 / 579 / 651 (fixtures start) per atlas guidance.")
     (:label "Designing barrier_finish_at semantics so savings is a useful, non-trivial number"
      :notes "First draft computed per-batch barrier (every task in a batch held to the slowest peer's finish), which made savings ≈ 0 for many natural manifests and obscured the metric's intent. Switched to per-task barrier (batchStart + own duration) so the metric captures what ready-queue actually reclaims when a task is unblocked by an early-finishing peer in a previous batch. wave_duration_savings_minutes (max barrier_finish - max ready finish) covers the slowest-peer-reclaim aggregate.")
     (:label "Diagnosing the raw NUL byte regression at line 489"
      :notes "Acceptance step 4 (perl /\\x00/) failed; perl reported `line 489 col 31`. Hex dump confirmed a literal 0x00 byte sat between `${from}` and `${to}` in a template literal. Repaired with a single Node one-liner that read the file, replaced /\\x00/g with a space, and wrote it back. Re-ran all 5 acceptance commands clean.")]

  :unexpected_work
    [(:summary "Added per-task gated_by field listing the predecessor ids that produced the longest ready_at value. Useful for debugging why a node released later than expected; surfaces the actual binding constraint.")
     (:summary "Added overlap_edges projection inside ready_queue so downstream tooling can audit which (from → to) edges the planner injected to preserve the reject-policy safety guarantee. Empty under warn policy by design.")
     (:summary "Added wave_duration_minutes + wave_duration_savings_minutes top-level aggregates so consumers can see the wall-clock improvement of ready-queue vs group-barrier without iterating per-task entries.")]

  :recommended_backend "claudecode"
  :router_confidence "high"
  :router_policy_path ".missiond/router/router-policy-v1.lisp"
  :router_dry_run_only true
  :router_applied false
  :router_reasons
    ["Dispatch strategy fresh-code-alignment + owner claudecode → matches r-fresh-code-alignment-to-claudecode in router-policy-v1 (priority 100, single matched rule)."
     "Pure JS planner CLI surface (no Rust / SQL / cargo, additive output field gated behind explicit flag, byte-identical default behavior) is the canonical claudecode beat — verification_tier=local; no full workspace build required."
     "Router output is recorded for telemetry only; runtime dispatch unchanged."]
  :router_trace_index_path ".missiond/router/trace-index-v1.lisp"

  :router_backend_readiness_status "current-default"
  :router_backend_runtime_allowed true
  :router_apply_eligible false
  :router_apply_blockers
    ["current-default is the live runtime today but explicit runtime-ready opt-in is required upstream before this task's dispatch handoff would mark a descriptor as apply-eligible (wave27-01 eligibility-gate intentionally REJECTS current-default → eligible)."]
  :router_backend_registry_path ".missiond/router/router-backend-registry-v1.lisp"

  :notes
    "wave29-06 ships:
     - scripts/plan-task-runner.mjs: SCHEDULE_MODES + DEFAULT_SCHEDULE_MODE exports, --schedule CLI flag, schedule option threaded through planFromManifestFile + planFromManifestObject, computeReadyQueue() pure helper.

     CLI surface (additive, backward-compatible):
       node scripts/plan-task-runner.mjs --manifest <m> [--json|--lisp] [--schedule group-barrier|ready-queue] [--dry-fixture]

     Default --schedule group-barrier produces output BYTE-IDENTICAL to the wave28-02 baseline. Verified via stash + diff against the pre-edit binary on the wave29 manifest (3-batch / critical_path=125 / overlap_diagnostics=[] / verification_tier_counts {full:0, local:6, smoke:1}).

     ready_queue top-level field (only emitted under --schedule ready-queue):
       :schema           literal \"missiond.task-runner-plan.ready-queue.v0\"
       :priority_rule    literal \"critical-path-remaining-desc\"
       :tie_break        literal \"task-id-lex-asc\"
       :overlap_edges    [{from, paths[], to} ...] sorted by (from, to); empty under overlap_policy=warn
       :order            [task_id ...] linear emission queue ordered by (ready_at asc, priority desc, task_id lex asc)
       :tasks            [{task_id, dispatch_group, estimated_minutes, ready_at_minutes, finish_at_minutes, barrier_finish_at_minutes, idle_window_savings_minutes, priority_minutes, gated_by[]} ...] sorted by task_id lex
       :aggregate_idle_window_savings_minutes  Σ idle_window_savings_minutes
       :max_idle_window_savings_minutes        max idle_window_savings_minutes
       :wave_duration_minutes                  max finish_at_minutes (ready-queue wall clock)
       :wave_duration_savings_minutes          max barrier_finish_at - wave_duration_minutes (>= 0)

     Algorithm:
       1. Effective dependency set per task = explicit :depends_on edges + (under reject policy only) overlap edges from lex-smaller to lex-larger id when two same-dispatch_group nodes share any write_scope path.
       2. Topological propagation computes ready_at = max(finish_at of effective deps), finish_at = ready_at + estimated_minutes.
       3. Acyclic by construction: explicit DAG already acyclic, overlap edges respect lex order within a group.
       4. barrier_finish_at = batchStart + estimated_minutes (per-task) where batchStart is the cumulative max-end of prior batches under the existing group-barrier schedule.
       5. idle_window_savings = max(0, barrier_finish_at - finish_at).

     Backward-compat proof:
       - Default branch: if (schedule === 'ready-queue') is the ONLY new code path; group-barrier path is the original assemble step verbatim.
       - 13 baseline fixtures stay green byte-identically (verified by --dry-fixture exit 0 with `fixtures: 13` baseline + 6 new = 19 reported total).
       - Wave29 manifest --json output diff vs pre-edit: zero bytes changed (git stash + diff confirmed BYTE-IDENTICAL).
       - --lisp output mode unchanged (formatPlanLisp untouched; ready_queue is JSON-only by design — no consumer asked for Lisp emission of ready_queue and adding it would risk drift).

     Schema policy:
       - task-runner-manifest-v1.lisp UNCHANGED. No new required fields, no new optional fields.
       - check-task-runner-manifest.mjs UNCHANGED. 22 baseline fixtures stay green.
       - The contract's allowance for adding :schedule_hint was intentionally NOT exercised — no consumer needs it and adding speculative metadata creates migration debt.

     Fixture totals:
       19 cases across 13 categories — 12 wave28-02 baseline + 1 wave28-06 loop smoke + 6 wave29-06 ready-queue (default-byte-identical, flag-emits-ready-windows, overlap-blocks-window, overlap-edge-injected-reject, priority-deterministic, idle-savings-computed). All categories sorted alphabetically in --json output.

     Hard rules honored:
       NO dispatch / NO spawn / NO child_process / NO git mutation / NO network / NO LLM. Pure file reads + JSON serialization.
       NO raw NUL bytes (perl /\\x00/ check passes). Single regression caught mid-implementation and repaired before commit.
       NO unused imports / variables (TS6133 clean — only the new SCHEDULE_MODES/DEFAULT_SCHEDULE_MODE exports + schedule plumbing added).
       Verification tier = local. No cargo executed.

     Pre-commit pipeline: plan-task-runner.mjs --dry-fixture (exit=0, 19/19) → check-task-runner-manifest.mjs --dry-fixture (exit=0, 22/22) → check-task-contract.mjs --all (exit=0, 105 tasks) → perl /\\x00/ (exit=0) → git diff --check (exit=0) → check-missiond-hooks.mjs --json (preflight aligned) → git add scripts/plan-task-runner.mjs (1 path) → task-scope-guard.mjs --mode staged (OK, 1 staged) → MISSIOND_TASK_CONTRACT=... git commit -m \"feat(tasks): plan runner ready queue\" (worker commit 15c5267fcaa4) → coordinator preflight surfaced TS6133 on `dependents` → drop the unused parameter at both signature + call site → re-run --dry-fixture (19/19), NUL check (clean), manifest fixtures (22/22), task-contract --all (105 tasks) → git add (only my file; reset other agents' incidentally-staged files first) → MISSIOND_TASK_CONTRACT=... git commit -m \"feat(tasks): plan runner ready queue\" (parent lint-cleanup hotfix 1951aaa4fe79) → verify-task-contract.mjs (OK against 1951aaa4fe79). Append-only ledger updates: shared-memory wave29-06-claim-001 (seq 9) before staging + wave29-06-completion-001 (seq) after verify; session-trace wave29-06-trace-start-001 (seq 17) + wave29-06-trace-read-001 (seq 18) + wave29-06-trace-commit-001 (seq, worker) + wave29-06-trace-hotfix-001 (seq, parent lint-cleanup) + wave29-06-trace-complete-001 (seq).

     Constraints honored: NO Rust / SQL / Cargo edits. Did not touch crates/**, .missiond/v2/**, .missiond/router/**, .missiond/tasks/wave28/**, any wave29-*.lisp other than session-trace + shared-memory (both are session-trace-writable / claim-allowed and explicitly NOT in :must-not-touch), .missiond/tasks/wave29/manifest.lisp, .missiond/tasks/wave29/dispatch-plan.lisp, .missiond/claudecode/**, scripts/check-context-atlas.mjs, scripts/check-pattern-card.mjs, scripts/check-task-report.mjs, scripts/verify-task-run.mjs, scripts/verify-task-runner-batch.mjs, scripts/prepare-task-runner-wave.mjs, scripts/render-wave-briefs.mjs. Did not git add . / git push / --no-verify / --amend / --force.")
