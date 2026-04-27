;; Wave 24 / Task 02 — Session trace corpus index v0.
;; Schema: missiond.report-contract.v1
;; Source: .missiond/tasks/wave24/wave24-02-trace-corpus-index-v0.lisp

(report wave24-02-trace-corpus-index-v0
  :schema "missiond.report-contract.v1"
  :task_id "wave24-02-trace-corpus-index-v0"
  :status done
  :commit_hash "e61088b8d300"
  :files_changed
    ["scripts/build-session-trace-index.mjs"]

  :acceptance_results
    [(:command "node scripts/build-session-trace-index.mjs --dry-fixture"
      :exit_code 0
      :ok true
      :notes "build-session-trace-index fixtures OK (7 cases, 7 categories): pass-clean (single-task trace, totals/events=5/tasks=1/backends=1/waves=1/commits=1, zero bottleneck_tags), pass-multi-wave (two trace files with task ids wave22-01-example and wave23-02-example aggregate into 2 by_wave buckets keyed by wave22/wave23), pass-bottleneck-tags (4×500_000ms+2 failures+3 retries fires long-running, high-retry, many-failures tags at task and corpus level), pass-no-completion (lone dispatch event with no complete fires no-completion tag), pass-multi-backend (claudecode/codex-orchestrator/patch-worker each get their own by_backend bucket; backend buckets carry empty bottleneck_tags by design — backends are never tagged because failures may span tasks), pass-empty (zero-input corpus emits all 8 top-level keys with zero counts), pass-deterministic (stableStringify is idempotent and top-level keys are alphabetically sorted in JSON output). Each fixture writes a temp .lisp under os.tmpdir(), parses via the exported parseTraceEvents helper from check-session-trace.mjs, runs pure buildIndex() over the parsed traces, and asserts on the resulting structured object. The tmp dir is unconditionally rm'd at the end of runFixtures.")
     (:command "node scripts/build-session-trace-index.mjs --json .missiond/tasks/wave23/session-trace.lisp"
      :exit_code 0
      :ok true
      :notes "Emitted stable JSON with top-level keys (alphabetically sorted): bottleneck_tags=[], by_backend={codex-orchestrator:{events=1,kinds.observation=1,...}}, by_task={wave23-01-session-trace-schema-v0:{events=1,...}}, by_wave={wave23:{events=1,...}}, schema=missiond.session-trace.v1, source_files=[{path,wave=wave23,schema,traces=1,events=1}], thresholds={high_retry=3,long_running_ms=1800000,many_failures=2}, totals={backends=1,commits=0,events=1,files=0,tasks=1,total_duration_ms=0,traces=1,waves=1}. wave23 trace ledger contained only the bootstrap observation event so all per-bucket counts are 1; no bottleneck_tags fire. JSON byte-stable across re-runs (verified via stableStringify idempotence in pass-deterministic fixture).")
     (:command "node scripts/check-session-trace.mjs --dry-fixture"
      :exit_code 0
      :ok true
      :notes "session-trace fixtures OK (22 cases, 18 categories) — unchanged from wave23-01/wave23-06. check-session-trace.mjs was NOT modified by this task; existing exports (parseTraceEvents, SCHEMA, KIND_VALUES, ENTRY_HEAD) were sufficient to build the indexer.")
     (:command "node scripts/check-task-contract.mjs --all"
      :exit_code 0
      :ok true
      :notes "task-contract check OK (66 tasks). The wave24-02 contract itself plus all other contracts validate; no schema changes in this task.")
     (:command "git diff --check -- scripts/build-session-trace-index.mjs scripts/analyze-session-trace.mjs scripts/check-session-trace.mjs"
      :exit_code 0
      :ok true
      :notes "git diff --check clean across the three in-scope script paths; the indexer is a fresh file (no whitespace deltas) and analyze-session-trace.mjs / check-session-trace.mjs were not edited.")
     (:command "node scripts/task-scope-guard.mjs --task .missiond/tasks/wave24/wave24-02-trace-corpus-index-v0.lisp --mode staged"
      :exit_code 0
      :ok true
      :notes "task-scope-guard staged OK: wave24-02-trace-corpus-index-v0 (1 staged file(s)) — the single staged path scripts/build-session-trace-index.mjs is inside :write-scope; zero matches against :must-not-touch (crates/** .missiond/v2/** .missiond/tasks/schema/*.lisp .missiond/tasks/wave23/** .missiond/tasks/wave24/wave24-*.lisp .missiond/claudecode/**).")
     (:command "node scripts/verify-task-contract.mjs .missiond/tasks/wave24/wave24-02-trace-corpus-index-v0.lisp"
      :exit_code 0
      :ok true
      :notes "task-contract verify OK: wave24-02-trace-corpus-index-v0 against e61088b8d300 — commit hash exists; commit message matches contract :commit.message exactly; changed_files ⊆ write-scope (write-scope-only); changed_files ∩ must-not-touch = ∅; acceptance commands present in contract.")
     (:command "node scripts/check-missiond-hooks.mjs --json"
      :exit_code 0
      :ok true
      :notes "ok=true severity=ok matches=true reason=aligned — core.hooksPath==.githooks already set from prior waves; .githooks/pre-commit exists and is executable; no install needed.")]

  :scope_deviations []

  :trace_refs [wave24-02-trace-start-001 wave24-02-trace-commit-001 wave24-02-trace-complete-001]

  :unexpected_work
    [(:summary "Parallel agent wave24-01 (router-policy schema) ran concurrently and appended its commit (988f7d88b467) and complete events to wave24 session-trace.lisp between my :start (seq 6) and :commit (seq 9) appends. This is the expected interleaving pattern: I re-read the file fresh after my own work and chose seq 9/10 to remain strictly increasing past their seq 7/8. Same pattern in shared-memory.lisp where their completion landed at seq 6 before my completion at seq 7. Both files validate (check-session-trace 10 events OK; check-task-memory 7 entries OK).")
     (:summary "analyze-session-trace.mjs and check-session-trace.mjs are listed in :write-scope but were NOT modified — the existing parseTraceEvents export from check-session-trace.mjs was sufficient and adding to analyze-session-trace.mjs would have introduced redundant abstractions. Deliberately did not touch them so their --dry-fixture suites remain byte-identical (verified post-write: analyze-session-trace 6 cases OK; check-session-trace 22 cases OK).")]

  :notes
    "Indexer scope (deliberate):
- Read-only by design: NO fs.writeFile / fs.appendFile / fs.writeFileSync / fs.appendFileSync / fs.mkdtempSync calls anywhere on the production CLI path. The only fs writes in the file are scoped to runFixtures (the --dry-fixture branch) — fs.mkdtempSync for an OS tmp dir + fs.writeFileSync for fixture trace files; the tmp dir is unconditionally rm'd via fs.rmSync at end-of-fixtures. Production main() path goes parseTraceEvents → buildIndex → console.log/console.error only.
- Glob scan via Node fs.readdirSync recursion (findSessionTraceFiles helper). Does NOT depend on shell `**` expansion. Default scan root is `.missiond/tasks` resolved against process.cwd(); skips .git and node_modules; matches files literally named `session-trace.lisp`. Output sorted alphabetically for stable file ordering.
- Stable JSON: top-level keys sorted alphabetically (bottleneck_tags / by_backend / by_task / by_wave / schema / source_files / thresholds / totals). Per-bucket keys also sorted alphabetically. KIND_VALUES enumerated in sorted order in each kinds dict so absent kinds are still represented as 0. stableStringify(value, indent=2) recursively sorts object keys (arrays preserve order; we already sort arrays where order matters: source_files by path, files/commits ascending). Verified deterministic in pass-deterministic fixture.

Top-level fields emitted by `--json`:
  bottleneck_tags (array of corpus-wide tag strings, deduped + sorted),
  by_backend{<backend-id>:{events,kinds{<kind>:n},command_count,test_count,
    failure_count,retry_count,dispatch_count,complete_count,commit_event_count,
    total_duration_ms,files_touched,files,commits,bottleneck_tags=[]}},
  by_task{<task-id>:{...same shape; bottleneck_tags computed per-task}},
  by_wave{<wave-id>:{...same shape; bottleneck_tags computed per-wave}},
  schema=missiond.session-trace.v1,
  source_files[{path,wave,schema,traces,events}] sorted by path,
  thresholds{high_retry,long_running_ms,many_failures},
  totals{backends,commits,events,files,tasks,total_duration_ms,traces,waves}.

Bottleneck tag rules (match wave23-06 analyze-session-trace.mjs thresholds exactly so corpus-level and per-trace analyses are comparable):
- long-running:   total_duration_ms >= 1_800_000  (30 minutes)
- high-retry:     retry_count       >= 3
- many-failures:  failure_count     >= 2
- no-completion:  dispatch_count    >= 1 AND complete_count == 0
Tags are computed at task and wave granularity; backend buckets are NOT tagged because a backend's failures may straddle many tasks and tagging would imply a router recommendation (out of scope per contract).

Wave grouping: extracted from task id prefix via /^(wave\\d+)\\b/. Falls back to the trace header's :wave when the task id does not match (defensive; current corpus has no such cases). waveOfTask(taskId) is exported for tooling.

Non-goals (explicit):
- Does NOT recommend router policies, model swaps, or backend changes (that is wave24-03 territory). The indexer only projects observed facts.
- Does NOT execute traced commands or replay anything.
- Does NOT mutate any ledger or write any output file in the CLI path.
- Does NOT extend analyze-session-trace.mjs or check-session-trace.mjs (the existing parseTraceEvents export from check-session-trace.mjs covers all needs; both prior scripts' --dry-fixture suites still pass byte-identically post-task).

Read-only proof:
- `grep -n 'fs.writeFile\\|fs.appendFile\\|fs.writeFileSync\\|fs.appendFileSync\\|fs.mkdtemp' scripts/build-session-trace-index.mjs` returns 3 hits: line 28 (header comment documenting the non-goal), line 580 (fs.mkdtempSync inside runFixtures), line 587 (fs.writeFileSync inside runFixtures). Both code-path hits live exclusively under the --dry-fixture branch. Production CLI path is provably free of file writes.
- main() function only calls findSessionTraceFiles (read-only readdirSync), parseTraceEvents (read-only readFileSync via lib), buildIndex (pure), and emit (console.log only). No imports of child_process, http, https, dns, net, or any other side-effecting module.

CLI surface:
- `node scripts/build-session-trace-index.mjs` — scan default `.missiond/tasks` recursively for session-trace.lisp files, emit human-readable summary to stdout.
- `--json` — emit deterministic JSON instead.
- `--dry-fixture` — run 7 self-contained pass cases (clean / multi-wave / bottleneck-tags / no-completion / multi-backend / empty / deterministic).
- positional `<trace.lisp> ...` — explicit file list (overrides default scan root). Files must exist or the script exits with code 2.
- `-h` / `--help` — print usage and exit 0.

Workflow protocol (wave24 conventions followed):
- Hooks preflight: aligned (core.hooksPath==.githooks); checked via `node scripts/check-missiond-hooks.mjs --json` returning ok=true severity=ok matches=true reason=aligned. No install needed.
- Shared-memory ledger entries appended (claim seq=5 → completion seq=7; parallel wave24-01 agent inserted seq=4 claim and seq=6 completion between them, which is the expected interleaving pattern documented in feedback_execution_lisp_protocol). Each entry uses unique :id (wave24-02-claim-001 / wave24-02-completion-001) and :touched lists only my :write-scope paths.
- Session-trace ledger entries appended (start seq=6, commit seq=9 with :commit_hash e61088b8d300..., complete seq=10). Re-read the file fresh between my own appends to pick up parallel agent's seq 7/8 (their commit + complete for wave24-01).
- Pre-commit task-scope-guard (--mode staged) reported 1 staged file, inside :write-scope, zero matches against :must-not-touch.
- MISSIOND_TASK_CONTRACT env var set on the git commit invocation; the .githooks/pre-commit hook re-ran the same guard and confirmed the staged scope.
- Post-commit verify: scripts/verify-task-contract.mjs cross-checked commit e61088b8d300 against the contract — message prefix `feat(tasks): index session trace corpus`, changed_files ⊆ write-scope, must-not-touch ∩ ∅, all green.

Constraints honored: did NOT touch crates/**; did NOT touch .missiond/v2/**; did NOT touch .missiond/tasks/schema/*.lisp; did NOT touch .missiond/tasks/wave23/**; did NOT touch other wave24 contracts (.missiond/tasks/wave24/wave24-*.lisp); did NOT touch .missiond/claudecode/**. The wave24 ledger edits and this report file are intentionally out-of-scope of the commit and remain untracked for the next wave's archive task per the established protocol. NO git add . / git stash / git reset / git checkout / amend / push / --no-verify / git add -A.")
