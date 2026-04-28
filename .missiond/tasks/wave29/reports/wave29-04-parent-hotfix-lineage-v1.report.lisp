;; Wave 29 / Task 04 — Parent hotfix lineage v1.
;; Schema: missiond.report-contract.v1
;; Source: .missiond/tasks/wave29/wave29-04-parent-hotfix-lineage-v1.lisp

(report wave29-04-parent-hotfix-lineage-v1
  :schema "missiond.report-contract.v1"
  :task_id "wave29-04-parent-hotfix-lineage-v1"
  :status done
  :commit_hash "d7e314ab86d6"
  :agent_commit_hash "c037c63f91aa"
  :final_commit_hash "d7e314ab86d6"
  :verified_commit_hash "d7e314ab86d6"
  :parent_patches
    [(:commit "d7e314ab86d6"
      :kind lint-cleanup
      :reason "TS6133 unused TRACE_SCHEMA import cleanup; pre-existing wave23-03 import that was never used in the file."
      :files ["scripts/verify-task-run.mjs"])]
  :files_changed
    [".missiond/tasks/schema/report-contract-v1.lisp"
     "scripts/check-task-report.mjs"
     "scripts/verify-task-run.mjs"
     "scripts/verify-task-runner-batch.mjs"]

  :acceptance_results
    [(:command "node scripts/check-task-report.mjs --dry-fixture"
      :exit_code 0
      :ok true
      :notes "task-report fixtures OK (48 cases). Wave29-04 v1 hardening added 6 new fixtures on top of the wave29-prep baseline (42): wave28-02 lineage exemplar (worker 954116e513c5 + parent lint-cleanup 302330a, with :commit_hash equal to last :parent_patches entry), fail-missing-commit (parent patch entry omits :commit), fail-malformed-hash (non-hex chars in :agent_commit_hash), fail-drift (commit_hash and last parent patch commit disagree), fail-bad-files-path (parent patch :files contains absolute path), fail-bad-kind (parent patch :kind not in enum). Existing 42 wave23/wave25/wave26/wave27/wave29-prep fixtures stay green byte-identically.")
     (:command "node scripts/verify-task-run.mjs --dry-fixture"
      :exit_code 0
      :ok true
      :notes "task-run verify fixtures OK (23 fixtures, 20 helper cases). Wave29-04 added 4 lineage fixtures (commit_hash + agent + parent_patches all aligned to final; agent_commit_hash matches resolved worker commit; resolved commit not in any lineage hash → structured failure; legacy report without lineage fields still verifies via commit_hash) plus 5 new buildLineageEntries helper cases. The existing 19 wave23-03 fixtures + 15 wave23-03 helper cases stay green byte-identically.")
     (:command "node scripts/verify-task-runner-batch.mjs --dry-fixture"
      :exit_code 0
      :ok true
      :notes "task-runner-batch verify fixtures OK (12 fixtures). Wave29-04 added 3 lineage fixtures (memory summary cites final commit 302330a; memory summary cites worker commit 954116e and verified row resolves to final; memory summary cites a hash outside lineage → fail with commit-hash-mismatch). Existing 9 wave28-05/wave28-06 fixtures stay green byte-identically. Either-hash matching is implemented by widening the report lineage hash collection to include every :parent_patches[i].commit on top of the four flat hash fields.")
     (:command "node scripts/check-task-report.mjs --all"
      :exit_code 0
      :ok true
      :notes "task-report check OK (68 reports) — all real on-disk wave22..wave28 reports including the wave28-02 hotfix exemplar with :agent_commit_hash 954116e513c5 + :parent_patches lint-cleanup entry validate green under the v1 hardening. Backward compatibility is non-negotiable and was verified.")
     (:command "node scripts/check-task-contract.mjs --all"
      :exit_code 0
      :ok true
      :notes "task-contract check OK (105 tasks) — all wave22..wave29 task contracts including wave29-04 parse and pass shape / scope / must-not-touch / acceptance / commit-policy validation. No regressions.")
     (:command "git diff --check -- scripts/check-task-report.mjs scripts/verify-task-run.mjs scripts/verify-task-runner-batch.mjs .missiond/tasks/schema/report-contract-v1.lisp"
      :exit_code 0
      :ok true
      :notes "no whitespace errors across the 4 in-scope paths.")
     (:command "node scripts/check-missiond-hooks.mjs --json"
      :exit_code 0
      :ok true
      :notes "preflight OK; core.hooksPath aligned to .githooks; .githooks/pre-commit exists and is executable; no install required.")
     (:command "node scripts/task-scope-guard.mjs --task .missiond/tasks/wave29/wave29-04-parent-hotfix-lineage-v1.lisp --mode staged"
      :exit_code 0
      :ok true
      :notes "task-scope-guard staged OK: wave29-04-parent-hotfix-lineage-v1 (4 staged file(s)) — all 4 staged paths inside :write-scope; zero matches against :must-not-touch (crates/** .missiond/v2/** .missiond/router/** schemas task-contract/task-runner-manifest/context-atlas/pattern-card .missiond/tasks/wave28/** .missiond/tasks/wave29/wave29-*.lisp manifest dispatch-plan .missiond/claudecode/** sibling scripts check-context-atlas.mjs check-pattern-card.mjs check-verification-receipt.mjs prepare-task-runner-wave.mjs render-wave-briefs.mjs plan-task-runner.mjs).")
     (:command "node scripts/verify-task-contract.mjs .missiond/tasks/wave29/wave29-04-parent-hotfix-lineage-v1.lisp"
      :exit_code 0
      :ok true
      :notes "task-contract verify OK after parent hotfix: final commit d7e314ab86d6 preserves the same task message/scope and changes only scripts/verify-task-run.mjs (TS6133 unused TRACE_SCHEMA import cleanup); original worker commit was c037c63f91aa. Commit lineage is recorded in :agent_commit_hash / :final_commit_hash / :verified_commit_hash / :parent_patches and exercises the v1 hardening this very task introduced — the report's own lineage is the canonical regression pin.")]

  :scope_deviations []

  :trace_refs [wave29-04-trace-start-001 wave29-04-trace-read-001 wave29-04-trace-commit-001 wave29-04-trace-complete-001]

  :major_decisions
    [(:decision "Schema and checker keep the four flat hash fields (:commit_hash, :agent_commit_hash, :final_commit_hash, :verified_commit_hash) AND the :parent_patches vector — wave29 prep had already wired all four; v1 hardening adds enum + drift + extended hex-format constraints rather than removing fields."
      :rationale "The wave28-02 real on-disk report depends on all four fields. Removing :final_commit_hash / :verified_commit_hash would have broken --all backward compatibility. The brief said 2 NEW optional fields but in practice the schema already had 4; the v1 hardening tightens validation rather than reshaping the surface.")
     (:decision "Parent-patch :kind enum is closed: lint-cleanup | doc-fix | test-fix | scope-trim | hotfix-other. Adding new kinds requires schema bump + checker update."
      :rationale "Open enums silently widen and make downstream tooling brittle. The wave28-02 hotfix uses lint-cleanup which is in the enum; the brief enumerates exactly these five kinds.")
     (:decision "Final-hash drift rule is anchored on the LAST entry of :parent_patches, not all entries. The intermediate parent patches can be different commits (chain of hotfixes), but the trailing entry MUST equal the report's :commit_hash."
      :rationale "The wave28-02 model is one worker commit + one parent hotfix → final commit is the trailing parent patch commit. Multi-hotfix chains where every patch progressively adjusts would still satisfy: each patch adds its commit; the final report :commit_hash matches the LAST entry. shaPrefixAgrees handles short-vs-long SHA prefix gracefully.")
     (:decision "verify-task-run lineage acceptance: any role in (commit_hash, agent_commit_hash, parent_patches[*].commit) can match the resolved git commit, but the JSON output explicitly tags WHICH role matched via lineage_match.role + .index."
      :rationale "Operators sometimes verify against a worker commit (CI run captured pre-hotfix) and sometimes against the final commit; either should pass without the verifier silently swallowing lineage info. The structured lineage_match field gives downstream tooling a deterministic handle to know if they are looking at a worker, agent, or hotfix verification.")
     (:decision "verify-task-runner-batch widens report-lineage harvesting to include every :parent_patches[i].commit (in addition to the four flat hash fields). Verified row still points at verifiedCommitHash ?? finalCommitHash ?? commitHash — i.e., the FINAL commit, never an intermediate."
      :rationale "Brief requirement #4: completion summaries written post-worker-commit but pre-hotfix legitimately quote the worker hash; the verifier must accept this. But the verified result row should always resolve to the final commit so downstream consumers see a single canonical hash.")]

  :time_sinks
    [(:label "Reading the existing wave29-prep scaffolding (schema + checker + verifier + batch verifier)"
      :notes "The wave29 prep work had already added :agent_commit_hash, :final_commit_hash, :verified_commit_hash, :parent_patches to the schema + checker + verifier + batch verifier with a baseline of 42 + 19+15 + 9 fixtures. Confirming what was already in place before adding v1 hardening saved duplication.")
     (:label "Designing the kind enum + drift rule + extended hex format trio"
      :notes "All three are simple individually but interact: the drift rule needs the same hex-prefix-agree semantics as wave23-03, the kind enum needs the bare-atom keywordPropText extraction (which works automatically), and the extended hex format (>=7, <=64) needs to apply to both top-level lineage hashes AND :parent_patches[i].commit consistently. Centralized via COMMIT_HASH_REGEX + ALLOWED_PARENT_PATCH_KINDS + shaPrefixAgrees so future edits stay coherent.")
     (:label "Wiring lineage exposure into verify-task-run JSON output"
      :notes "Surfacing :agent_commit_hash + :parent_patches in the JSON output without breaking the existing 19+15 fixtures meant adding a new check (report_commit_lineage) that always populates rather than gating on lineage presence. The role/index tagging in lineage_match makes failure messages debuggable too.")]

  :unexpected_work
    [(:summary "Extended buildReportSource in verify-task-runner-batch.mjs to accept agentCommitHash + parentPatches options. The existing helper only took commit_hash; without the extension the new lineage fixtures would have had to inline raw Lisp source strings (brittle and noisy). Also exported collectReportLineageHashes for downstream tooling reuse.")
     (:summary "Added 5 buildLineageEntries helper cases to verify-task-run dry fixtures so the canonical role ordering (commit_hash → agent_commit_hash → parent_patches[*]) and de-duplication semantics (when commit_hash equals the trailing parent_patch commit, only one entry is emitted) are pinned at the helper level rather than only via the integration fixtures.")
     (:summary "Detected that the wave29-prep schema already had :final_commit_hash + :verified_commit_hash even though the brief mentions 2 NEW fields. Confirmed that the wave28-02 on-disk report uses all four — removing two fields would have broken --all backward compatibility. v1 hardening adds enum + drift + extended hex-format constraints on top.")]

  :recommended_backend "claudecode"
  :router_confidence "high"
  :router_policy_path ".missiond/router/router-policy-v1.lisp"
  :router_dry_run_only true
  :router_applied false
  :router_reasons
    ["Dispatch strategy fresh-code-alignment + owner claudecode → matches r-fresh-code-alignment-to-claudecode in router-policy-v1 (priority 100, single matched rule)."
     "Workstation surface (Node.js CLI hardening across 3 verifier scripts plus 1 schema doc; no Rust / SQL / cargo) is the canonical claudecode beat — pure file edits, no network / LLM call required."
     "Router output is recorded for telemetry only; runtime dispatch unchanged (claudecode is the live default and remained the live default for this task)."]
  :router_trace_index_path ".missiond/router/trace-index-v1.lisp"

  :router_backend_readiness_status "current-default"
  :router_backend_runtime_allowed true
  :router_apply_eligible false
  :router_apply_blockers
    ["current-default is the live runtime today but explicit runtime-ready opt-in is required upstream before this task's dispatch handoff would mark a descriptor as apply-eligible (wave27-01 eligibility-gate intentionally REJECTS current-default → eligible)."]
  :router_backend_registry_path ".missiond/router/router-backend-registry-v1.lisp"

  :notes
    "wave29-04 hardens parent-hotfix commit lineage from the wave29 prep scaffolding to v1.

     Schema (.missiond/tasks/schema/report-contract-v1.lisp):
       - Documented :agent_commit_hash hex format (>= 7 and <= 64 chars).
       - Documented :parent_patches structured contract: :commit hex SHA, :kind atom enum (lint-cleanup | doc-fix | test-fix | scope-trim | hotfix-other), :reason non-empty string, :files non-empty repo-relative vector.
       - Documented final-hash drift invariant: when :commit_hash and the LAST :parent_patches entry's :commit are both present, they MUST agree (hex prefix-match >= 7 chars).
       - Updated checker-contract :rejects list with the four new failure modes.

     check-task-report.mjs:
       - Added ALLOWED_PARENT_PATCH_KINDS Set with the five canonical kinds.
       - Added COMMIT_HASH_REGEX = /^[0-9a-f]{7,64}$/i (extended from the wave29-prep /^[0-9a-f]{7,40}$/).
       - Added shaPrefixAgrees helper for the drift rule.
       - validateCommitLineage: enforces hex format on all 3 top-level hashes, enforces final/verified hash equality with :commit_hash, validates each :parent_patches entry (commit hex, kind enum, reason non-empty, files non-empty + repo-relative + no .. traversal), and enforces final-hash drift on the trailing entry.
       - Fixtures: 42 → 48 (+6: wave28-02 lineage exemplar, fail-missing-commit, fail-malformed-hash, fail-drift, fail-bad-files-path, fail-bad-kind).

     verify-task-run.mjs:
       - reportFromForms now extracts :parent_patches into a flat structured array (commit, kind, reason, files).
       - verifyRun now builds a canonical lineage entries array (commit_hash → agent_commit_hash → parent_patches[*].commit, de-duplicated) via buildLineageEntries.
       - check.report_commit_hash now accepts ANY lineage hash matching the resolved git commit, surfacing the matched role + index in lineage_match. Failure error message names the lineage explicitly.
       - check.report_commit_lineage is a new always-populated structured field exposing :agent_commit_hash + :final_commit_hash + :verified_commit_hash + :parent_patches in the JSON output.
       - Fixtures: 19 → 23 (+4 lineage cases). Helper cases: 15 → 20 (+5 buildLineageEntries cases).

     verify-task-runner-batch.mjs:
       - Added collectReportLineageHashes export — widens the wave29-prep four-flat-field harvest to also include every :parent_patches[i].commit. Memory completion summaries that mention the worker commit OR the final commit OR an intermediate parent-patch commit all match.
       - Verified result row still resolves to verifiedCommitHash ?? finalCommitHash ?? commitHash — i.e., the FINAL commit, never an intermediate (cross-wave invariant from the brief: 'verified result should point at the final/verified hash').
       - Extended buildReportSource fixture helper to accept agentCommitHash + parentPatches (so synthetic fixtures stay readable).
       - Fixtures: 9 → 12 (+3: memory cites final, memory cites worker, memory cites hash outside lineage).

     Wave28-02 lineage exemplar verified: worker commit 954116e513c5 + parent lint-cleanup commit 302330a is pinned in check-task-report.mjs fixture 'wave29-04 wave28-02 lineage exemplar (worker 954116e513c5 + parent lint-cleanup 302330a)' which matches the on-disk wave28-02 report shape verbatim. The on-disk wave28-02 report itself was NOT modified (it lives in wave28's archive and is out of scope); the fixture mirrors its structure as a regression pin.

     Backward compatibility (load-bearing): all 42 pre-existing check-task-report fixtures stay green byte-identically; all 19+15 verify-task-run fixtures stay green; all 9 verify-task-runner-batch fixtures stay green; --all real-reports check (68 reports across wave22..wave28 including the wave28-02 hotfix exemplar) stays green; --all task-contract check (105 tasks) stays green; git diff --check zero whitespace errors.

     Read-only invariant honored: no git mutation introduced in any verifier; the FORBIDDEN_GIT_VERBS allowlist in verify-task-run.mjs continues to reject add/commit/reset/push/checkout/etc. The batch verifier's readCommitAt path is unchanged (still git rev-parse + git log + git show via verify-task-contract.readCommit).

     Pre-commit pipeline:
       check-task-report.mjs --dry-fixture (exit=0, 48/48)
       -> verify-task-run.mjs --dry-fixture (exit=0, 23 fixtures + 20 helpers)
       -> verify-task-runner-batch.mjs --dry-fixture (exit=0, 12/12)
       -> check-task-report.mjs --all (exit=0, 68 reports)
       -> check-task-contract.mjs --all (exit=0, 105 tasks)
       -> git diff --check (exit=0)
       -> check-missiond-hooks.mjs --json (preflight aligned)
       -> git add 4 in-scope files
       -> task-scope-guard.mjs --mode staged (OK, 4 staged)
       -> MISSIOND_TASK_CONTRACT=... git commit -m 'feat(tasks): verify parent hotfix lineage' (commit c037c63f91aa)
       -> verify-task-contract.mjs against commit c037c63f91aa (OK).

     Append-only ledger updates: shared-memory wave29-04-claim-001 + wave29-04-completion-001; session-trace wave29-04-trace-start-001 + wave29-04-trace-read-001 + wave29-04-trace-commit-001 + wave29-04-trace-complete-001. Both ledgers re-validated after each append; fresh tail-read before each append because parallel wave29-01/02 agents appended their own claim/start/commit/completion entries between this task's claim and completion.

     Constraints honored: NO Rust / SQL / Cargo edits. Did not touch crates/**, .missiond/v2/**, .missiond/router/**, schemas (task-contract / task-runner-manifest / context-atlas / pattern-card), .missiond/tasks/wave28/**, any wave29-*.lisp other than session-trace + shared-memory + reports/wave29-04-*.report.lisp (all session-trace-writable / claim-allowed / report-output and explicitly NOT in :must-not-touch), .missiond/tasks/wave29/manifest.lisp, .missiond/tasks/wave29/dispatch-plan.lisp, .missiond/claudecode/**, scripts/check-context-atlas.mjs, scripts/check-pattern-card.mjs, scripts/check-verification-receipt.mjs, scripts/prepare-task-runner-wave.mjs, scripts/render-wave-briefs.mjs, scripts/plan-task-runner.mjs. Did not git add . / git push / --no-verify / --amend / --force.")
