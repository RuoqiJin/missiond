;; Wave 24 / Task 03 — Router recommendation CLI v0.
;; Schema: missiond.report-contract.v1
;; Source: .missiond/tasks/wave24/wave24-03-router-recommendation-cli-v0.lisp

(report wave24-03-router-recommendation-cli-v0
  :schema "missiond.report-contract.v1"
  :task_id "wave24-03-router-recommendation-cli-v0"
  :status done
  :commit_hash "d6d8e102e1aa"
  :files_changed
    ["scripts/recommend-task-backend.mjs"
     "scripts/check-router-policy.mjs"]

  :acceptance_results
    [(:command "node scripts/recommend-task-backend.mjs --dry-fixture"
      :exit_code 0
      :ok true
      :notes "recommend-task-backend fixtures OK (10 cases, 10 categories): pass-matched-high (matching rule + rich trace evidence yields backend=claudecode confidence=high chosen_rule_id=r-docs-to-claudecode dry_run_only=true non_goals carried from matched rule), pass-matched-medium (matching rule + sparse 1..4 events yields confidence=medium), pass-matched-low (matching rule + empty trace index yields confidence=low and evidence.trace_events_total=0), pass-fallback (kind=ops off-policy task yields backend=claudecode confidence=low matched.length=0 evidence.reason=insufficient_trace_history chosen_rule_id=null), pass-priority-ordering (two rules match at priority 5 and 50; matched_rules contains BOTH in priority-ascending order but chosen_rule_id is the priority-5 rule and backend comes from THAT rule's :recommend), pass-rejected-explain (the seed-style 3-rule policy rejects 2 rules each with non-empty failing_clause string and actual string for reviewer audit), pass-invariant (dry_run_only is literal true in the JSON output even when matched), pass-deterministic (stableStringify is idempotent and top-level keys are alphabetically sorted), edge-malformed-task (a Lisp form that is not (task ...) throws with `no \\(task ...\\) form` rather than silently returning a fallback recommendation — fail-fast per global brief), edge-runtime-replacement (a policy declaring :runtime-replacement true is recognized via projectPolicy and the cross-wave invariant guard would reject it before any matching).")
     (:command "node scripts/recommend-task-backend.mjs --task .missiond/tasks/wave24/wave24-01-router-policy-schema-v1.lisp --policy .missiond/router/router-policy-v1.lisp --json"
      :exit_code 0
      :ok true
      :notes "Emitted stable JSON with 12 top-level keys (alphabetically sorted): backend=deterministic-checker, chosen_rule_id=r-deterministic-checker-tasks, confidence=low (no trace-index supplied), dry_run_only=true, evidence={backend_event_count=0,bottleneck_tags=[],task_event_count=0,trace_events_total=0}, matched_rules=[{id:r-deterministic-checker-tasks,priority:20,reasoning:'When the only acceptance bar is a deterministic checker script, ...'}], non_goals=3 strings deduped+sorted from the matched rule, policy_path=absolute path, rejected_rules=[{id:r-docs-to-claudecode,priority:10,failing_clause:'(kind docs)',actual:'code-alignment'},{id:r-post-commit-verifier,priority:30,failing_clause:'(kind review)',actual:'code-alignment'}], schema=missiond.router-recommendation.v0, task_id=wave24-01-router-policy-schema-v1, task_path=absolute path. The wave24-01 contract has kind=code-alignment and write-scope includes scripts/check-router-policy.mjs which matches the path-glob 'scripts/check-*.mjs' inside the (all (kind code-alignment) (path-glob ...)) clause of r-deterministic-checker-tasks; the docs and review rules are rejected on (kind ...) mismatch with their failing_clause and actual surfaced. Output is byte-stable across re-runs.")
     (:command "node scripts/check-router-policy.mjs .missiond/router/router-policy-v1.lisp"
      :exit_code 0
      :ok true
      :notes "router-policy check OK (1 policy, 3 rules) — the wave24-01 seed policy still validates after the named-export additions to scripts/check-router-policy.mjs. The CLI surface (main, runFixtures, validatePolicy, validateRule) is unchanged.")
     (:command "node scripts/check-router-policy.mjs --dry-fixture"
      :exit_code 0
      :ok true
      :notes "router-policy fixtures OK (19 cases, 13 categories) — byte-identical to wave24-01 baseline. The new exports (projectRule, projectPolicy, readRouterPolicyFile) live below the runFixtures function and are pure projections; they do not change validation semantics or fixture results.")
     (:command "node scripts/check-task-contract.mjs --all"
      :exit_code 0
      :ok true
      :notes "task-contract check OK (66 tasks). The wave24-03 contract itself plus all other contracts validate; no schema changes in this task.")
     (:command "git diff --check -- scripts/recommend-task-backend.mjs scripts/check-router-policy.mjs scripts/build-session-trace-index.mjs"
      :exit_code 0
      :ok true
      :notes "git diff --check clean across the three in-scope script paths. recommend-task-backend.mjs is a new file; check-router-policy.mjs has additive named exports + import-meta gate adjustments; build-session-trace-index.mjs is unchanged.")
     (:command "node scripts/task-scope-guard.mjs --task .missiond/tasks/wave24/wave24-03-router-recommendation-cli-v0.lisp --mode staged"
      :exit_code 0
      :ok true
      :notes "task-scope-guard staged OK: wave24-03-router-recommendation-cli-v0 (2 staged file(s)) — both staged paths (scripts/recommend-task-backend.mjs and scripts/check-router-policy.mjs) are inside :write-scope; zero matches against :must-not-touch (crates/** .missiond/v2/** .missiond/tasks/schema/*.lisp .missiond/tasks/wave23/** .missiond/tasks/wave24/wave24-*.lisp .missiond/claudecode/**).")
     (:command "node scripts/verify-task-contract.mjs .missiond/tasks/wave24/wave24-03-router-recommendation-cli-v0.lisp"
      :exit_code 0
      :ok true
      :notes "task-contract verify OK: wave24-03-router-recommendation-cli-v0 against d6d8e102e1aa — commit hash exists; commit message matches contract :commit.message exactly ('feat(tasks): recommend backend from trace policy'); changed_files ⊆ write-scope (write-scope-only); changed_files ∩ must-not-touch = ∅; acceptance commands present in contract.")
     (:command "node scripts/check-missiond-hooks.mjs --json"
      :exit_code 0
      :ok true
      :notes "ok=true severity=ok matches=true reason=aligned — core.hooksPath==.githooks already set from prior waves; .githooks/pre-commit exists and is executable; no install needed.")
     (:command "node scripts/build-session-trace-index.mjs --dry-fixture"
      :exit_code 0
      :ok true
      :notes "build-session-trace-index fixtures OK (7 cases, 7 categories) — byte-identical to wave24-02 baseline. The wave24-03 CLI imports buildIndex from this module but does NOT modify it; existing exports were sufficient.")]

  :scope_deviations []

  :trace_refs [wave24-03-trace-start-001 wave24-03-trace-commit-001 wave24-03-trace-complete-001]

  :unexpected_work
    [(:summary "Originally listed scripts/build-session-trace-index.mjs in :write-scope and the initial claim, but the file's existing buildIndex export was already sufficient — adding more exports would have been gold-plating. Updated the shared-memory claim summary to record the actual touched set ({recommend-task-backend.mjs, check-router-policy.mjs}) and committed only those two files. The task contract's :write-scope still lists the third path; that is a permitted-but-unused entry, not a violation. task-scope-guard runs in mode=staged so absent files do not trigger errors.")]

  :notes
    "Recommendation algorithm (deterministic, no LLM):
1. Parse task contract (projectTaskContract) and router policy (projectPolicy) — both are pure projections over Lisp forms via the shared scripts/lib/missiond_lisp.mjs reader. Rules are sorted by :priority ascending so first-match wins by lowest priority value.
2. Defensive cross-wave guard (BEFORE any matching): the CLI exits non-zero if the policy declares :runtime-replacement true OR is missing :dry-run-only true. The wave24-01 checker also rejects such policies, but the recommendation CLI re-checks so it is independently safe.
3. For each rule in priority order, evaluate :when via evaluateClause (predicate evaluator). The 8 predicate heads from wave24-01 schema are implemented exactly: kind / dispatch_strategy / dispatch-strategy / owner / status (exact-match against contract field; underscore and kebab variants of dispatch_strategy treated as the same predicate), path-glob (true if ANY path in :write-scope matches the glob via globToRegExp from missiond_lisp.mjs — `**` matches across `/`, `*` does not), any (logical OR; first failure surfaced when none pass), all (logical AND; first failure surfaced). The top-level :when list is implicit-`all`.
4. Collect matched_rules (priority-ascending order) and rejected_rules (priority-ascending order, each with structured failing_clause string + observed actual value).
5. Selection: chosen rule = matched[0]. backend = chosen rule's :recommend.backend. reasoning = chosen rule's :recommend.reasoning. Other matched rules are recorded in matched_rules for explainability but do NOT influence backend.
6. non_goals: aggregated across ALL matched rules, deduped (via Set), sorted alphabetically for deterministic output.
7. confidence: high if max(task_event_count, backend_event_count) >= 5 (RICH_TRACE_THRESHOLD); medium if 1..4; low if 0 OR no rule matched OR no trace-index supplied.
8. No-match fallback: backend=claudecode (FALLBACK_BACKEND), confidence=low, evidence.reason=insufficient_trace_history (FALLBACK_REASON), chosen_rule_id=null. Fail-fast per global brief — this is NOT a silent default; the reason field surfaces the fallback to reviewers.
9. dry_run_only is HARD-CODED literal true on every output (cross-wave invariant). Not derived from policy, not configurable.

Output JSON shape (top-level keys, alphabetically sorted):
- backend (string, one of the 5 backend classes)
- chosen_rule_id (string or null)
- confidence (string: high|medium|low)
- dry_run_only (boolean: ALWAYS true)
- evidence ({backend_event_count, bottleneck_tags[], task_event_count, trace_events_total, reason?})
- matched_rules (array of {id, priority, reasoning} in priority-ascending order)
- non_goals (string array, deduped+sorted)
- policy_path (string, absolute path)
- rejected_rules (array of {id, priority, failing_clause, actual} in priority-ascending order)
- schema (string: 'missiond.router-recommendation.v0')
- task_id (string from task contract)
- task_path (string, absolute path)

Named exports added to scripts/check-router-policy.mjs (CLI surface unchanged):
- projectRule(rule) — project a single (rule ...) form into a structured object {id, priority, when:clause, recommend:{backend,reasoning}, non_goals[], notes, loc}. Returns null if the form is not a rule.
- projectPolicy(form, sourcePath?) — project a (router-policy ...) form into {id, schema, version, dry_run_only, runtime_replacement, description, rules[], source_path}. Rules are sorted by priority ascending. Booleans projected from literal Lisp atoms.
- readRouterPolicyFile(file) — read a router-policy file from disk and return the FIRST projected policy form. Throws with file context if no router-policy form is present.
- (also: helper projectClause and projectWhenList are NOT exported; they are private to projectRule.)
- (BACKEND_CLASSES and PREDICATE_HEADS were already exported by wave24-01.)
- check-router-policy.mjs --dry-fixture still passes byte-identically (19 cases / 13 categories); the additions sit below runFixtures and do not modify validation logic.

scripts/build-session-trace-index.mjs was NOT modified. The wave24-03 CLI imports the existing buildIndex named export and uses it in a fixture (emptyTraceIndex) to demonstrate stable shape compatibility between the two CLIs. The wave24-02 --dry-fixture suite (7 cases) still passes byte-identically post-task.

Hard guarantee — read-only, no side effects:
- grep -E 'child_process|spawn|fetch|http|https|exec|git' scripts/recommend-task-backend.mjs returns 2 hits, both in COMMENTS:
  • line 24: header comment documenting the invariant ('NO shell-out, NO LLM, NO HTTP, NO git')
  • line 316: substring of 'legitimately' inside a comment about non-goal de-duplication
  No active code matches: zero child_process imports, zero http/https/fetch calls, zero git invocations, zero spawn/exec calls.
- File reads via fs.readFileSync (task contract, policy file, optional trace index JSON) and via the shared readLispFile helper. NO file writes in production CLI path. The runFixtures() function does not write files (unlike wave24-02 which uses tmp dirs); fixtures run entirely in-memory via parseLisp + projectTaskContract + projectPolicy.

CLI surface:
- node scripts/recommend-task-backend.mjs --task <task.lisp> --policy <router-policy.lisp> [--trace-index <index.json>] [--json] [--dry-fixture]
- --task and --policy are REQUIRED unless --dry-fixture is set; missing required flags exit with code 2 and usage.
- --trace-index is optional; without it, evidence counts are 0 and confidence falls to low for matched rules.
- --json emits stable JSON (keys sorted, byte-identical across re-runs).
- Without --json, a human-readable summary is emitted (task / backend / confidence / matched rules / rejected rules / non-goals / dry_run_only).
- -h / --help prints usage and exits 0.
- Unknown flags exit with code 2 and usage; this is fail-fast per global brief — do not silently accept malformed args.

Workflow protocol (wave24 conventions followed):
- Hooks preflight: aligned (core.hooksPath==.githooks); checked via `node scripts/check-missiond-hooks.mjs --json` returning ok=true severity=ok matches=true reason=aligned. No install needed.
- Shared-memory ledger entries appended (claim seq=8 → completion seq=9). Each entry uses unique :id (wave24-03-claim-001 / wave24-03-completion-001) and :touched lists only my actually-modified files.
- Session-trace ledger entries appended (start seq=11, commit seq=12 with :commit_hash d6d8e102e1aa, complete seq=13). All entries reference my prior trace ids in :trace_refs.
- Pre-commit task-scope-guard (--mode staged) reported 2 staged files, both inside :write-scope, zero matches against :must-not-touch.
- MISSIOND_TASK_CONTRACT env var set on the git commit invocation; the .githooks/pre-commit hook re-ran the same guard and confirmed the staged scope.
- Post-commit verify: scripts/verify-task-contract.mjs cross-checked commit d6d8e102e1aa against the contract — message exactly 'feat(tasks): recommend backend from trace policy', changed_files ⊆ write-scope, must-not-touch ∩ ∅, all green.

Constraints honored: did NOT touch crates/**; did NOT touch .missiond/v2/**; did NOT touch .missiond/tasks/schema/*.lisp; did NOT touch .missiond/tasks/wave23/**; did NOT touch other wave24 contracts (.missiond/tasks/wave24/wave24-*.lisp); did NOT touch .missiond/claudecode/**. The wave24 ledger edits and this report file are intentionally out-of-scope of the commit and remain untracked for the next wave's archive task per the established protocol. NO git add . / git stash / git reset / git checkout / amend / push / --no-verify / git add -A.")
