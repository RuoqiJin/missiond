;; Wave 25 / Task 01 — Router policy corpus evaluator v0.
;; Schema: missiond.report-contract.v1
;; Source: .missiond/tasks/wave25/wave25-01-router-policy-corpus-evaluator-v0.lisp

(report wave25-01-router-policy-corpus-evaluator-v0
  :schema "missiond.report-contract.v1"
  :task_id "wave25-01-router-policy-corpus-evaluator-v0"
  :status done
  :commit_hash "8dbe85fa1a0c"
  :files_changed
    ["scripts/evaluate-router-policy-corpus.mjs"]

  :acceptance_results
    [(:command "node scripts/evaluate-router-policy-corpus.mjs --dry-fixture"
      :exit_code 0
      :ok true
      :notes "evaluate-router-policy-corpus fixtures OK (8 cases, 8 categories): empty-corpus (totals.tasks=0, per_task.length=0, fallbacks=0, rejections=0 across two re-runs to confirm stability), multi-task-corpus (3 tasks: docs / checker / review match the 3 seed-style rules → by_backend = {claudecode:1, deterministic-checker:1, verifier-worker:1, missiond-llm-router:0, patch-worker:0} sums to 3; by_confidence sums to 3; per_task sorted by task_id), fallback-rows (1 ops + 1 docs task → fallback_count=1; the ops row carries matched_rule_id=null and fallback_reason=insufficient_trace_history and backend=claudecode and confidence=low), rejected-policy (a fixture policy declaring :runtime-replacement true is recognized via projectPolicy.runtime_replacement===true so the main() guard would exit non-zero before any matching — the fixture exercises the same logic without calling process.exit), rejected-task (a Lisp form that is NOT (task ...) under tasks-root → rejected_count=1, totals.rejections=1, the per_task row surfaces backend=null / confidence=null / fallback_reason='malformed_task_contract: ...' for reviewer audit; the loop continues past the malformed file and the second real task still recommends), deterministic-output (running the same fixture corpus through stableStringify twice yields byte-identical JSON; top-level keys are alphabetically sorted; schema=missiond.router-policy-evaluation.v0), trace-index-equivalence (the same per-task recommendation is produced when the trace index is supplied via --trace-index <json> vs built in-process via buildIndex; only trace_index_source differs; a 5-event synthesised trace yields confidence=high), self-audit (active source code contains zero call sites matching child_process / spawn / execSync / fork / openai / anthropic / chat.completion / fetch( / https.{get,request,post} after stripping line and block comments AND single/double/template string literals).")
     (:command "node scripts/evaluate-router-policy-corpus.mjs --policy .missiond/router/router-policy-v1.lisp --tasks-root .missiond/tasks --json"
      :exit_code 0
      :ok true
      :notes "Real-corpus run emits stable JSON with 11 top-level keys (alphabetically sorted): by_backend / by_confidence / fallback_count / per_task / policy_path / rejected_count / schema / tasks_root / totals / trace_index_source. totals = {tasks:67, fallbacks:43, rejections:0}. by_backend = {claudecode:49, deterministic-checker:14, missiond-llm-router:0, patch-worker:0, verifier-worker:4} (sums to 67; the two zero buckets prove schema-enum seeding works — neither backend is recommended by the seed policy yet). by_confidence = {high:6, medium:5, low:56} (sums to 67). policy_path = .missiond/router/router-policy-v1.lisp (repo-relative). tasks_root = .missiond/tasks. trace_index_source = built-in-process:.missiond/tasks (no --trace-index supplied so the evaluator stood up the index in-process via findSessionTraceFiles → parseTraceEvents → buildIndex over .missiond/tasks/wave23/session-trace.lisp + wave24 + wave25). per_task contains 67 rows sorted by task_id ascending; the 6 high-confidence rows are all wave-NN-00 archive tasks (kind=docs, matching r-docs-to-claudecode at priority 10 and the trace index's claudecode bucket has >=5 events). The 4 verifier-worker rows are wave-NN-NN smoke / autonomous-loop tasks (kind=smoke matching r-post-commit-verifier at priority 30). 14 deterministic-checker rows are scripts/check-*.mjs writers (kind=code-alignment + path-glob 'scripts/check-*.mjs' matching r-deterministic-checker-tasks at priority 20). 43 fallback rows have matched_rule_id=null and fallback_reason=insufficient_trace_history (kind=feature / ops / etc. that the seed policy does not yet cover).")
     (:command "node scripts/check-router-policy.mjs .missiond/router/router-policy-v1.lisp"
      :exit_code 0
      :ok true
      :notes "router-policy check OK (1 policy, 3 rules) — the wave24-01 seed policy still validates; wave25-01 does not modify the policy file or the checker.")
     (:command "node scripts/check-task-contract.mjs --all"
      :exit_code 0
      :ok true
      :notes "task-contract check OK (74 tasks). The wave25-01 contract itself plus all other contracts validate; no schema changes in this task.")
     (:command "git diff --check -- scripts/evaluate-router-policy-corpus.mjs"
      :exit_code 0
      :ok true
      :notes "git diff --check clean on the single new file. No whitespace errors; the file uses LF line endings consistent with the rest of scripts/.")
     (:command "node scripts/task-scope-guard.mjs --task .missiond/tasks/wave25/wave25-01-router-policy-corpus-evaluator-v0.lisp --mode staged"
      :exit_code 0
      :ok true
      :notes "task-scope-guard staged OK: wave25-01-router-policy-corpus-evaluator-v0 (1 staged file(s)) — the only staged path scripts/evaluate-router-policy-corpus.mjs is inside :write-scope; zero matches against :must-not-touch (crates/** scripts/recommend-task-backend.mjs scripts/build-session-trace-index.mjs scripts/check-router-policy.mjs .missiond/v2/** .missiond/tasks/schema/*.lisp .missiond/tasks/wave24/** .missiond/tasks/wave25/wave25-*.lisp .missiond/claudecode/**).")
     (:command "node scripts/verify-task-contract.mjs .missiond/tasks/wave25/wave25-01-router-policy-corpus-evaluator-v0.lisp"
      :exit_code 0
      :ok true
      :notes "task-contract verify OK: wave25-01-router-policy-corpus-evaluator-v0 against 8dbe85fa1a0c — commit hash exists; commit message matches contract :commit.message exactly ('feat(tasks): evaluate router policy over trace corpus'); changed_files ⊆ write-scope (write-scope-only); changed_files ∩ must-not-touch = ∅; acceptance commands present in contract.")
     (:command "node scripts/check-missiond-hooks.mjs --json"
      :exit_code 0
      :ok true
      :notes "ok=true severity=ok matches=true reason=aligned — core.hooksPath==.githooks already set from prior waves; .githooks/pre-commit exists and is executable; no install needed.")]

  :scope_deviations []

  :trace_refs [wave25-01-trace-start-001 wave25-01-trace-commit-001 wave25-01-trace-complete-001]

  :unexpected_work
    [(:summary "The brief listed 6-8 dry-fixture cases targeting empty corpus / multi-task / fallback / rejected policy / deterministic JSON / trace-index supplied vs built. Implemented 8 cases. Added an 8th 'self-audit' fixture that reads the evaluator's own source, strips line + block comments AND single/double/template string literals, then scans for forbidden patterns (child_process / spawn / execSync / fork / openai / anthropic / chat.completion / fetch( / https.{get,request,post}). The audit lives inside the dry-fixture so it runs every time the script is exercised, complementing the contract's grep-based audit step. The 'rejected policy' fixture mirrors recommend-task-backend.mjs's wave24-03 pattern and asserts the projector recognizes :runtime-replacement true rather than calling process.exit() inside the fixture (which would tear down the harness); the runtime CLI guard in main() does the actual non-zero exit.")]

  :notes
    "Read-only corpus evaluator. The wave24 chain (router-policy schema in wave24-01, trace corpus index in wave24-02, recommendation CLI in wave24-03) produced a per-task recommendation; this task lifts that to a per-corpus measurement so reviewers can see at-a-glance how the seed policy behaves over the entire wave19→wave25 task contract corpus.

Output schema (missiond.router-policy-evaluation.v0) top-level keys (alphabetically sorted):
- by_backend          : {<backend>: <count>, ...} — zero-seeded against the wave24-01 BACKEND_CLASSES enum so the report shape stays stable even when the corpus does not exercise every class.
- by_confidence       : {high: <n>, medium: <n>, low: <n>}.
- fallback_count      : count of tasks that fell back to claudecode/insufficient_trace_history (matched_rule_id===null OR backend===FALLBACK_BACKEND with reason===FALLBACK_REASON).
- per_task            : array of {backend, confidence, fallback_reason, matched_rule_id, task_id, task_path}, sorted by task_id ascending.
- policy_path         : repo-relative string.
- rejected_count      : count of tasks whose contract failed to parse.
- schema              : 'missiond.router-policy-evaluation.v0'.
- tasks_root          : repo-relative string.
- totals              : {tasks, fallbacks, rejections}.
- trace_index_source  : either 'file:<repo-relative-path>' or 'built-in-process:<repo-relative-tasks-root>'.

Algorithm (deterministic, no LLM):
1. Parse the router-policy via readRouterPolicyFile (named export from check-router-policy.mjs). Defensive cross-wave guard: exit non-zero if :runtime-replacement true OR :dry-run-only is missing/false. The wave24-03 CLI also re-checks this; we mirror it so a malformed policy fails the corpus run loudly rather than silently producing fallback rows for every task.
2. Load the trace index. If --trace-index <json> is supplied, fs.readFileSync + JSON.parse it. Otherwise, build it in-process by scanning --tasks-root for session-trace.lisp files via findSessionTraceFiles (build-session-trace-index named export), parsing each via parseTraceEvents (check-session-trace named export), then aggregating via buildIndex (build-session-trace-index named export). NEVER spawn the underlying CLIs.
3. Walk --tasks-root recursively for .lisp files. Skip:
   - schema/ directories (schema definitions, not contracts).
   - reports/ directories (machine-readable report ledgers).
   - shared-memory.lisp (per-wave append-only ledger).
   - session-trace.lisp (per-wave append-only trace).
   - *-parallel-dispatch-index.lisp (coordination shim, not a real task — wave22-09 and similar are kind=docs|coordination but explicitly out of corpus per brief).
4. For each surviving file, attempt to project it via readTaskContractFile (recommend-task-backend named export). On parser error: increment rejected_count, push a per_task row with backend=null / confidence=null / fallback_reason='malformed_task_contract: <message>' and continue. On success: call recommend({task, policy, traceIndex, taskPath, policyPath}) and bucket the result into by_backend / by_confidence; detect fallback via chosen_rule_id===null OR (backend===FALLBACK_BACKEND && reason===FALLBACK_REASON).
5. Sort per_task by task_id ascending. Emit JSON via stableStringify (recursive key sort) for byte-identical output across runs.

Real corpus result (commit 8dbe85fa1a0c, --policy .missiond/router/router-policy-v1.lisp --tasks-root .missiond/tasks):
- 67 tasks evaluated (the corpus excludes the 7 *-parallel-dispatch-index.lisp shims among the 74 task-contract files counted by check-task-contract --all; --all does count them because they are still legal contracts, but the brief explicitly carves them out of the policy-evaluation corpus).
- 0 rejections (every task contract on disk parses cleanly).
- 43 fallbacks (64 % of corpus): kinds the seed policy does not cover yet (feature / ops / autonomous / etc.).
- by_backend = {claudecode:49, deterministic-checker:14, verifier-worker:4, missiond-llm-router:0, patch-worker:0}: the 49 claudecode count = 6 high-confidence wave-NN-00 archive tasks + 43 fallback tasks, sums match.
- by_confidence = {high:6, medium:5, low:56}: high are wave-NN-00 archive tasks where the trace index has >=5 claudecode events; medium are recent code-alignment tasks where the trace index has 1..4 events for the recommended backend; low are everything else (mostly fallbacks, plus matched rules with empty trace-index buckets).
- The two zero-bucket backends (missiond-llm-router, patch-worker) are intentional — the seed policy from wave24-01 has 3 rules covering claudecode / deterministic-checker / verifier-worker only; the other 2 backend classes are reserved for future rules but currently never recommended. The zero-seeded buckets surface this directly in the report instead of forcing reviewers to infer it from missing keys.

Read-only / no-shell / no-git / no-LLM proof:
- Active source contains zero call sites matching child_process / spawn / execSync / fork / openai / anthropic / chat.completion / fetch( / https.{get,request,post} (verified by the self-audit dry-fixture which strips line + block comments AND single/double/template string literals before scanning, and verified manually via `grep -nE 'child_process|spawn|fetch|http|https|exec|git|openai|anthropic' scripts/evaluate-router-policy-corpus.mjs` — every hit is in a comment or string literal: lines 14/15/22/166 are header comments documenting the invariant; line 239 is the directory walker skipping `.git` and `node_modules` (NOT a git invocation); line 767 is the fixture name 'audit: no shell / git / LLM / HTTP call sites in active source'; lines 793-800 are the audit's own RegExp literals).
- File reads via fs.readFileSync (policy file, optional trace-index JSON), fs.readdirSync (corpus walker), and the shared readLispFile / parseTraceEvents helpers. NO file writes in production CLI path (--json prints to stdout). The runFixtures() function uses fs.mkdtempSync inside os.tmpdir() and fs.rmSync({recursive:true,force:true}) in finally for cleanup — no production files touched.
- ZERO git invocation. The directory walker only filters out a directory named '.git' (the canonical .git/ subdirectory) and 'node_modules' to avoid descending into them; no git CLI / git library / shell-out occurs.
- Cross-wave invariant preserved: the recommend() pipeline emits dry_run_only=true on every per-task recommendation (literal true, not configurable). The corpus evaluator does not mutate this; the per_task rows in the report are the rec.backend / rec.confidence / rec.chosen_rule_id projection only — the full dry_run_only=true literal is implicit in the schema name 'missiond.router-policy-evaluation.v0' and surfaced explicitly via the per-task call into recommend() which always carries it.

CLI surface:
- node scripts/evaluate-router-policy-corpus.mjs --policy <path> [--tasks-root <dir>] [--trace-index <path>] [--json] [--dry-fixture]
- --policy is REQUIRED unless --dry-fixture is set; missing required flag exits with code 2 and usage.
- --tasks-root defaults to .missiond/tasks (DEFAULT_SCAN_ROOT, imported from build-session-trace-index for parity).
- --trace-index is optional; when omitted the evaluator builds the index in-process from session-trace.lisp files under --tasks-root.
- --json emits stable JSON (keys sorted, byte-identical across re-runs).
- Without --json, a human-readable summary is emitted (policy / tasks_root / trace_index / totals / by_backend / by_confidence / per_task).
- -h / --help prints usage and exits 0.
- Unknown flags exit with code 2 and usage; this is fail-fast per global brief — do not silently accept malformed args.

Imports (named exports, never shell-out):
- recommend-task-backend.mjs: recommend, readTaskContractFile, FALLBACK_BACKEND, FALLBACK_REASON.
- build-session-trace-index.mjs: buildIndex, findSessionTraceFiles, DEFAULT_SCAN_ROOT.
- check-router-policy.mjs: BACKEND_CLASSES, readRouterPolicyFile.
- check-session-trace.mjs: parseTraceEvents.
None of these scripts were modified by wave25-01; the named exports were added in wave24-01 / wave24-02 / wave24-03 / wave23-06 specifically to enable downstream tooling like this evaluator.

Workflow protocol (wave24/wave25 conventions followed):
- Hooks preflight: aligned (core.hooksPath==.githooks); checked via `node scripts/check-missiond-hooks.mjs --json` returning ok=true severity=ok matches=true reason=aligned. No install needed.
- Shared-memory ledger entries appended (claim seq=4 → completion seq=8). Each entry uses unique :id (wave25-01-claim-001 / wave25-01-completion-001) and :touched lists only my actually-modified file. Sibling agents (wave25-02 / wave25-03) appended their own claims (seqs 5/6) and a completion (seq 7) concurrently; my appends do not conflict with theirs.
- Session-trace ledger entries appended (start seq=5, commit seq=10 with :commit_hash 8dbe85fa1a0c, complete seq=11). Sibling agents appended their own start (seq 6/7) and wave25-02 commit/complete (seqs 8/9) concurrently; sequence numbers are unique and ordered.
- Pre-commit task-scope-guard (--mode staged) reported 1 staged file inside :write-scope, zero matches against :must-not-touch.
- MISSIOND_TASK_CONTRACT env var set on the git commit invocation; the .githooks/pre-commit hook re-ran the same guard and confirmed the staged scope.
- Post-commit verify: scripts/verify-task-contract.mjs cross-checked commit 8dbe85fa1a0c against the contract — message exactly 'feat(tasks): evaluate router policy over trace corpus', changed_files ⊆ write-scope, must-not-touch ∩ ∅, all green.

Constraints honored: did NOT touch crates/**; did NOT touch scripts/recommend-task-backend.mjs / scripts/build-session-trace-index.mjs / scripts/check-router-policy.mjs (only IMPORTed via named exports); did NOT touch .missiond/v2/**; did NOT touch .missiond/tasks/schema/*.lisp; did NOT touch .missiond/tasks/wave24/**; did NOT touch other wave25 contracts (.missiond/tasks/wave25/wave25-*.lisp); did NOT touch .missiond/claudecode/**. The wave25 ledger edits and this report file are intentionally out-of-scope of the commit and remain untracked for the next wave's archive task per the established protocol. NO git add . / git stash / git reset / git checkout / amend / push / --no-verify / git add -A.")
