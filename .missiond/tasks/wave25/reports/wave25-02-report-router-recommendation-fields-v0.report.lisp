;; Wave 25 / Task 02 — report-contract router recommendation fields v0.
;; Schema: missiond.report-contract.v1
;; Source: .missiond/tasks/wave25/wave25-02-report-router-recommendation-fields-v0.lisp

(report wave25-02-report-router-recommendation-fields-v0
  :schema "missiond.report-contract.v1"
  :task_id "wave25-02-report-router-recommendation-fields-v0"
  :status done
  :commit_hash "770903136f00"
  :files_changed
    [".missiond/tasks/schema/report-contract-v1.lisp"
     "scripts/check-task-report.mjs"]

  :acceptance_results
    [(:command "node scripts/check-task-report.mjs --dry-fixture"
      :exit_code 0
      :ok true
      :notes "task-report fixtures OK (22). The 16 wave23-02 fixtures (valid done report / valid draft / missing task_id / invalid status / empty acceptance / abs path / malformed acceptance / schema mismatch / done without commit / scope_deviations abs path / valid worker-explanation block / time_sinks shape / major_decisions missing :decision / unexpected_work missing :summary / blockers empty string / trace_refs abs) keep passing byte-identically; the 6 wave25-02 additions (legacy / valid block / invalid backend / applied=true / dry_run_only=false / abs policy path) all match expected outcomes.")
     (:command "node scripts/check-task-report.mjs .missiond/tasks/wave24/reports/wave24-04-plan-router-dry-run-surface-v0.report.lisp"
      :exit_code 0
      :ok true
      :notes "task-report check OK (1 report) — the wave24-04 daemon dry-run report has no router_recommendation fields and validates cleanly under the new optional-field rules, proving backward compatibility.")
     (:command "node scripts/check-task-contract.mjs --all"
      :exit_code 0
      :ok true
      :notes "task-contract check OK (74 tasks). Schema edit to report-contract-v1.lisp does not affect task-contract validation (different schema namespace). All 74 contracts including the wave25-02 contract itself parse and validate.")
     (:command "git diff --check -- .missiond/tasks/schema/report-contract-v1.lisp scripts/check-task-report.mjs"
      :exit_code 0
      :ok true
      :notes "git diff --check clean across both write-scope files; no trailing whitespace, mixed tabs/spaces, or conflict markers.")
     (:command "node scripts/task-scope-guard.mjs --task .missiond/tasks/wave25/wave25-02-report-router-recommendation-fields-v0.lisp --mode staged"
      :exit_code 0
      :ok true
      :notes "task-scope-guard staged OK: wave25-02-report-router-recommendation-fields-v0 (2 staged file(s)). Both staged paths inside :write-scope; zero matches against :must-not-touch (crates/** / scripts/render-claudecode-task.mjs / scripts/recommend-task-backend.mjs / scripts/evaluate-router-policy-corpus.mjs / .missiond/v2/** / .missiond/tasks/wave24/** / .missiond/tasks/wave25/wave25-*.lisp / .missiond/claudecode/**).")
     (:command "node scripts/verify-task-contract.mjs .missiond/tasks/wave25/wave25-02-report-router-recommendation-fields-v0.lisp"
      :exit_code 0
      :ok true
      :notes "task-contract verify OK against 770903136f00 — commit hash exists; commit message exactly matches contract :commit.message ('feat(tasks): record router recommendation in reports'); changed_files ⊆ write-scope (write-scope-only); changed_files ∩ must-not-touch = ∅; acceptance commands present in contract.")
     (:command "node scripts/check-missiond-hooks.mjs --json"
      :exit_code 0
      :ok true
      :notes "ok=true severity=ok matches=true reason=aligned — core.hooksPath==.githooks already set; .githooks/pre-commit exists and is executable; no install needed.")]

  :scope_deviations []

  :trace_refs [wave25-02-trace-start-001 wave25-02-trace-commit-001 wave25-02-trace-complete-001]

  :recommended_backend "claudecode"
  :router_confidence "low"
  :router_policy_path ".missiond/router/router-policy-v1.lisp"
  :router_dry_run_only true
  :router_applied false
  :router_reasons ["dispatch-strategy:fresh-code-alignment"
                   "owner:claudecode"
                   "kind:code-alignment"
                   "fallback:insufficient_trace_history"]

  :major_decisions
    [(:decision "Chose Option A flat top-level fields over Option B nested :router_recommendation block"
      :rationale "Brief recommended Option A; matches the existing flat layout of wave23-02's worker-explanation fields (:time_sinks / :major_decisions / etc.) and avoids introducing a new property-list key/value-shape that the checker would have to descend into. Each field stays independently optional so partial recordings (e.g. just :recommended_backend + :router_confidence on an early report) are still valid."
      :trace_ref "wave25-02-trace-start-001")
     (:decision "Implemented validateRouterLiteralBool as STRICT atom-only check rather than reusing keywordPropBool"
      :rationale "keywordPropBool coerces the strings \"true\" / \"yes\" / \"on\" to booleans, which would let a report sneak past the cross-wave invariant by writing :router_applied \"false\" (a quoted string). The strict literal check enforces the exact daemon-emitted bareword shape — node.type === 'atom' && node.value === 'false' — mirroring the daemon helper that hard-codes the literal Bool(false) in the JSON block."
      :trace_ref "wave25-02-trace-start-001")
     (:decision "Added \"..\" traversal rejection to :router_policy_path / :router_trace_index_path validators"
      :rationale "The brief asked for absolute / leading-~ rejection (mirroring :trace_refs from wave23-02). I extended that to reject any \"..\" segment because the daemon's std::fs::read_to_string is otherwise vulnerable to a report claiming \"..\\..\\..\\etc\\passwd\" as the policy path. Belt-and-braces: the daemon already canonicalizes paths, but the report layer should never RECORD an out-of-repo path either."
      :trace_ref "wave25-02-trace-commit-001")
     (:decision "Did NOT change ALLOWED_ROUTER_BACKENDS or ALLOWED_ROUTER_CONFIDENCE to import from a shared module"
      :rationale "scripts/recommend-task-backend.mjs is on the must-not-touch list, and creating a NEW shared module would violate the strict 2-file write-scope. The two enums are duplicated as constants at the top of check-task-report.mjs with a comment pointing to the wave24-04 daemon module and wave24-03 CLI as the canonical sources. If the enum drifts, a future wave can extract a shared constants module then.")]

  :unexpected_work
    [(:summary "Discovered that keywordPropBool returns false for the string \"false\" (because nodeBool's normalizer maps \"false\"/\"no\"/\"off\" to false). Had to write a separate validateRouterLiteralBool that bypasses the helper entirely and checks node.type === 'atom' for the strict literal-atom requirement."
      :trace_ref "wave25-02-trace-start-001")
     (:summary "Confirmed parallel agent wave25-03 is also touching crates/missiond-daemon/src/handlers/knowledge/plan.rs (their write-scope, not mine). Their working-tree change appeared in git status alongside mine; I stage-listed only my two write-scope files to keep the commit clean."
      :trace_ref "wave25-02-trace-commit-001")]

  :notes
    "Surface choice: Option A (flat top-level keys) per the brief's recommendation.

Schema (.missiond/tasks/schema/report-contract-v1.lisp):
- :status string updated to mention the wave25-02 router-recommendation field group.
- (optional-report-fields ...) appended with seven new keys in a clearly-commented wave25-02 block.
- (field-contract ...) gained seven entries documenting purpose + accepted values + cross-wave invariants per field.
- (checker-contract ...) :rejects list extended with six new failure modes (enum, enum, literal-true, literal-false, abs-or-traversal path, vector-of-non-empty-strings) and :non-goal updated to clarify recommendations are observational only.

Checker (scripts/check-task-report.mjs):
- Two new module-level enum constants ALLOWED_ROUTER_BACKENDS / ALLOWED_ROUTER_CONFIDENCE pinned to the wave24-04 daemon and wave24-03 CLI shape (5 backends, 3 confidence levels).
- Four new validators wired into validateReport AFTER the wave23-02 worker-explanation block:
  * validateRouterEnumField: nullable; non-empty string; member of allowed set.
  * validateRouterRepoRelativePath: nullable; non-empty; rejects path.isAbsolute() / leading '~' / any '..' segment.
  * validateRouterLiteralBool: nullable; STRICT — node.type === 'atom' && node.value === expectedAtom (no string coercion). Cross-wave invariant for :router_applied=false / :router_dry_run_only=true.
  * validateRouterReasons: nullable; vector of non-empty strings (not property lists — daemon emits plain strings).
- Six new fixtures appended to runFixtures(): legacy report (no router fields, ok), valid full block (ok), invalid backend (rejected), :router_applied true (rejected), :router_dry_run_only false (rejected), absolute :router_policy_path (rejected).

Backward-compat proof:
- Pre-edit baseline: 16/16 fixtures pass (`node scripts/check-task-report.mjs --dry-fixture` showed `task-report fixtures OK (16)`).
- Post-edit: 22/22 fixtures pass; the names, sources, and ok/expects of the original 16 are byte-identical (no fixture renamed / re-ordered / re-keyed).
- Existing real-world reports: `node scripts/check-task-report.mjs --all` validates all 43 reports in .missiond/tasks/**/reports/*.report.lisp without a single new diagnostic — wave19..wave25-00 reports stay valid with no router fields.
- Smoke against wave24-04 (the daemon's own dry-run report which has no router fields): exit 0.

Cross-wave invariants enforced structurally:
- :router_applied MUST be the literal atom `false`. Strings, the literal atom `true`, and missing literal-bool atoms all yield: `:router_applied must be the literal atom false (cross-wave invariant)`.
- :router_dry_run_only MUST be the literal atom `true`. Same strictness.
- :recommended_backend MUST be one of {claudecode, missiond-llm-router, deterministic-checker, patch-worker, verifier-worker}.
- :router_confidence MUST be one of {high, medium, low}.
- :router_policy_path / :router_trace_index_path MUST be repo-relative (no '/' prefix, no '~', no '..' segment).
- :router_reasons MUST be a vector of non-empty strings.

Self-application: this very report demonstrates the new fields. :recommended_backend = claudecode (this task ran on claudecode), :router_confidence = low (no historical traces for this exact code-alignment task yet — wave25-01's evaluator will populate that as the corpus grows), :router_policy_path = the wave24-01 seed, :router_dry_run_only = true literal, :router_applied = false literal, :router_reasons = the four signals the wave24-03 recommend() would have surfaced, :router_trace_index_path omitted (no index consulted at report-time).

Constraints honored: did NOT touch crates/** (wave25-03's plan.rs change is theirs, not mine — I left it unstaged in my commit); did NOT touch scripts/render-claudecode-task.mjs (wave24-05 / wave25-04 territory); did NOT touch scripts/recommend-task-backend.mjs (wave24-03); did NOT touch scripts/evaluate-router-policy-corpus.mjs (wave25-01's NEW file); did NOT touch .missiond/v2/** or .missiond/tasks/wave24/** or .missiond/tasks/wave25/wave25-*.lisp or .missiond/claudecode/**. The only ledger appends are wave25 shared-memory.lisp + session-trace.lisp per the established protocol (claim/start before work, commit/complete/completion after) — these remain untracked for the next wave's archive task. NO git add . / git stash / git reset / git checkout / amend / push / --no-verify / git add -A.")
