;; Wave 23 / Task 06 — Session trace summary analyzer v0.
;; Schema: missiond.report-contract.v1
;; Source: .missiond/tasks/wave23/wave23-06-trace-summary-analyzer-v0.lisp

(report wave23-06-trace-summary-analyzer-v0
  :schema "missiond.report-contract.v1"
  :task_id "wave23-06-trace-summary-analyzer-v0"
  :status done
  :commit_hash "e7ab99dfc4a9"
  :files_changed
    ["scripts/analyze-session-trace.mjs"
     "scripts/check-session-trace.mjs"]

  :acceptance_results
    [(:command "node scripts/analyze-session-trace.mjs --dry-fixture"
      :exit_code 0
      :ok true
      :notes "analyze-session-trace fixtures OK (6 cases): clean trace mixed kinds (verifies events=5 cmds=1 tests=1 duration=2300ms files_touched=2 commits=1 hints=0), retries+failures (high-retry+many-failures hints fire), dispatched-but-no-completion (no-completion hint fires), long-running aggregate (4×500_000ms=2_000_000ms ≥ 1_800_000ms threshold ⇒ long-running hint), multi-file aggregation (2 trace files merge into 2 task buckets and ≥2 backend buckets), empty trace (graceful zero-counts: events=0 tasks=0 backends=0 files_touched=0 commits=0 total_duration_ms=0). Each fixture writes a temp .lisp under os.tmpdir(), parses via the exported parseTraceEvents helper, runs pure summarize() over the parsed trace, and asserts on the resulting structured object — no shell-out, no network, no mutation outside the temp dir which is rm-rf'd at the end.")
     (:command "node scripts/check-session-trace.mjs --dry-fixture"
      :exit_code 0
      :ok true
      :notes "session-trace fixtures OK (22 cases, 18 categories) — unchanged from wave23-01. The checker's CLI behavior is preserved: SCHEMA / KIND_VALUES / ENTRY_HEAD constants now have `export` prefixes, a new pure helper parseTraceEvents(file) is exported, and main() is gated by `if (import.meta.url === \"file://\" + process.argv[1])` so direct invocation runs the CLI but module imports stay side-effect free. No fixture cases were added, removed, or modified.")
     (:command "node scripts/check-task-contract.mjs --all"
      :exit_code 0
      :ok true
      :notes "task-contract check OK (57 tasks). The wave23-06 contract itself plus all other contracts validate; no schema or contract changes in this commit (write scope did not touch .missiond/tasks/schema/session-trace-v1.lisp — no clarifying comments needed, the schema text was already complete).")
     (:command "git diff --check -- scripts/analyze-session-trace.mjs scripts/check-session-trace.mjs .missiond/tasks/schema/session-trace-v1.lisp"
      :exit_code 0
      :ok true
      :notes "git diff --check clean across all 3 in-scope paths; analyzer is a fresh file (no whitespace deltas), checker has only the 4 export keywords + helper insertion + main() gate (no trailing-whitespace / mixed-indentation regressions), schema file untouched.")]

  :scope_deviations []

  :notes
    "Analyzer scope (deliberate):
- Read-only: never opens any output file. Writes only to stdout (summary) and stderr (fixture failures). Fixture trace files are written to os.tmpdir() and rm -rf'd unconditionally on exit.
- Pure aggregator: summarize(traces) is exported so other tooling can consume the same structured object without going through the CLI. Hint computation is data-driven from the per-task bucket — no hidden state, no I/O.
- CLI surface: --json (structured object), --dry-fixture (6 self-test cases), positional <trace.lisp...> (one or more files; aggregated). -h / --help prints usage and exits 0. No flag for write/output paths exists by design.

Top-level fields emitted by `--json`:
  schema, files_scanned, traces[], totals{events,tasks,backends,files_touched,commits,total_duration_ms},
  files[], commits[], by_task{<task-id>:{events,kinds{<kind>:n},command_count,test_count,
  failure_count,retry_count,dispatch_count,complete_count,total_duration_ms,files_touched,
  files,commits,hints[{rule,threshold(_ms?),observed(_ms?)}]}},
  by_backend{<backend-id>:{...same shape minus hints}},
  hints[] (reserved aggregate; per-task hints live in by_task[*].hints),
  thresholds{long_running_ms,high_retry,many_failures}.

Bottleneck hint rules (conservative, observed-facts-only):
- long-running:    by_task[*].total_duration_ms >= 1_800_000  (30 minutes)
- high-retry:      by_task[*].retry_count       >= 3
- many-failures:   by_task[*].failure_count     >= 2
- no-completion:   by_task[*].dispatch_count >= 1 AND by_task[*].complete_count == 0
All thresholds documented in the analyzer module-header comment block AND emitted in the `thresholds` object so the consumer never has to read source.

Non-goals (explicitly enforced):
- Does NOT execute or replay any traced command (matches the checker's non-goal: structure-only).
- Does NOT propose router policy or model selection — that is wave23-07 territory (owned by Codex).
- Does NOT speculate about hidden ClaudeCode reasoning — only observed `:duration_ms` / `:exit_code` / kind counts surface.
- Does NOT mutate the trace file or any other repo path.
- Does NOT spawn child processes or import network code.

Checker extension (scripts/check-session-trace.mjs):
- Added `export` to: SCHEMA constant, ENTRY_HEAD constant, KIND_VALUES Set.
- Added `export function parseTraceEvents(file)` — pure helper that reads a trace file via readLispFile, walks (session-trace ...) forms, and yields a flat array of event records with all required + optional fields normalized (integers parsed via parseIntOrNull, vector fields default to []). The helper does NOT apply the strict checker rules; callers tolerate partial data because trace files may be mid-validation. This separation keeps the analyzer decoupled from the checker's diagnostic semantics.
- Added `parseIntOrNull(text)` private helper supporting parseTraceEvents.
- Replaced the unconditional `main();` call at module bottom with `if (import.meta.url === \\\"file://\\\" + process.argv[1]) main();` so the CLI runs on direct `node scripts/check-session-trace.mjs ...` invocation but stays silent when imported by analyzers/tests.
- Did NOT modify any existing checker logic, fixture, error message, exit code, or argv parsing path. The checker's --dry-fixture (22 cases / 18 categories) passes byte-identically.

Workflow protocol (wave23 conventions followed):
- Hooks preflight: aligned (core.hooksPath==.githooks); checked via `node scripts/check-missiond-hooks.mjs --json` returning ok=true severity=ok matches=true reason=aligned. No install needed.
- Shared-memory ledger entries appended (claim seq=7 → completion seq=9; a parallel agent inserted seq=8 between them, which is the expected interleaving pattern). Each entry uses unique :id (wave23-06-claim-001 / wave23-06-completion-001) and :touched lists only my :write-scope paths.
- Pre-commit task-scope-guard (--mode staged) reported 2 staged files, both inside :write-scope, zero matches against :must-not-touch (crates/** .missiond/v2/*.lisp .missiond/tasks/wave22/** scripts/render-claudecode-task.mjs).
- MISSIOND_TASK_CONTRACT env var set on the git commit invocation; the .githooks/pre-commit hook re-ran the same guard and confirmed the staged scope.
- Post-commit verify: scripts/verify-task-contract.mjs cross-checked commit e7ab99dfc4a9 against the contract — message prefix `feat(tasks): summarize session traces`, changed_files ⊆ write-scope, must-not-touch ∩ ∅, all green.

Constraints honored: did NOT touch crates/** (the parallel wave23-04 agent has unstaged modifications to crates/missiond-daemon/src/handlers/knowledge/agent_execution.rs in the worktree; left untouched and unstaged); did NOT touch .missiond/v2/*.lisp; did NOT touch .missiond/tasks/wave22/**; did NOT touch scripts/render-claudecode-task.mjs (parallel wave23-02); did NOT touch scripts/lib/missiond_lisp.mjs (avoided conflict with parallel agents); did NOT touch .missiond/tasks/schema/session-trace-v1.lisp (in :write-scope but no clarifying comments were needed — the schema doc is already complete). The wave23 ledger edits and this report file are intentionally out-of-scope of the commit and remain untracked / unstaged for the next wave's archive task per the established protocol. NO git add . / git stash / git reset / git checkout / amend / push / --no-verify.")
