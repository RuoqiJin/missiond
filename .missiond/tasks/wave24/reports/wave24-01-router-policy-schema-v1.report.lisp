;; Wave 24 / Task 01 — Router policy schema/checker v1.
;; Schema: missiond.report-contract.v1
;; Source: .missiond/tasks/wave24/wave24-01-router-policy-schema-v1.lisp

(report wave24-01-router-policy-schema-v1
  :schema "missiond.report-contract.v1"
  :task_id "wave24-01-router-policy-schema-v1"
  :status done
  :commit_hash "988f7d88b467"
  :files_changed
    [".missiond/router/router-policy-v1.lisp"
     ".missiond/tasks/schema/router-policy-v1.lisp"
     "scripts/check-router-policy.mjs"]

  :acceptance_results
    [(:command "node scripts/check-router-policy.mjs --dry-fixture"
      :exit_code 0
      :ok true
      :notes "19 fixture cases across 13 categories: 3 pass cases (minimal policy, all 5 backend classes enumerated, composite any/all predicates) and 16 fail-rule categories: fail-schema, fail-dry-run-only (missing + literal false), fail-runtime-replacement (literal true REJECTED + missing header), fail-duplicate-id, fail-duplicate-priority, fail-unknown-backend (gpt-5 rejected), fail-missing-non-goals (missing + empty []), fail-malformed-predicate (non-list clause + unknown head), fail-unknown-entry-head, fail-unknown-field, fail-recommend (missing :backend), fail-priority (non-integer). Cross-wave invariant — runtime-replacement-rejection — is exercised by the 'runtime-replacement true REJECTED' fixture in the fail-runtime-replacement category.")
     (:command "node scripts/check-router-policy.mjs .missiond/router/router-policy-v1.lisp"
      :exit_code 0
      :ok true
      :notes "Seed policy validates: 1 policy (seed-v1), 3 rules (r-docs-to-claudecode priority 10, r-deterministic-checker-tasks priority 20, r-post-commit-verifier priority 30). Header :dry-run-only true and :runtime-replacement false present and literal. Every rule lists :non-goals.")
     (:command "node scripts/check-task-contract.mjs --all"
      :exit_code 0
      :ok true
      :notes "66 task contracts across all waves remain green; this commit adds zero new task contracts (only schema + seed + checker), so total stays at 66.")
     (:command "git diff --check -- .missiond/tasks/schema/router-policy-v1.lisp .missiond/router/router-policy-v1.lisp scripts/check-router-policy.mjs"
      :exit_code 0
      :ok true
      :notes "No whitespace errors on the three write-scope paths.")
     (:command "node scripts/check-missiond-hooks.mjs --json"
      :exit_code 0
      :ok true
      :notes "Hooks preflight aligned: core.hooksPath == .githooks, .githooks/pre-commit present and executable; no install needed.")
     (:command "node scripts/task-scope-guard.mjs --task .missiond/tasks/wave24/wave24-01-router-policy-schema-v1.lisp --mode staged"
      :exit_code 0
      :ok true
      :notes "task-scope-guard staged OK: wave24-01-router-policy-schema-v1 (3 staged file(s)) — all 3 staged paths inside :write-scope; zero matches against :must-not-touch (crates/** .missiond/v2/** .missiond/tasks/wave23/** .missiond/tasks/wave24/wave24-*.lisp .missiond/claudecode/**).")
     (:command "MISSIOND_TASK_CONTRACT=.missiond/tasks/wave24/wave24-01-router-policy-schema-v1.lisp git commit -m \"feat(tasks): add router policy contract\""
      :exit_code 0
      :ok true
      :notes "Commit 988f7d88b467: 3 files, +1102 insertions, 0 deletions. Pre-commit scope-guard hook re-ran cleanly inside the commit (printed the same 'task-scope-guard staged OK' line before the [main 988f7d8] commit summary).")
     (:command "node scripts/verify-task-contract.mjs .missiond/tasks/wave24/wave24-01-router-policy-schema-v1.lisp"
      :exit_code 0
      :ok true
      :notes "task-contract verify OK: wave24-01-router-policy-schema-v1 against 988f7d88b467 — commit hash exists; subject equals contract :commit :message ('feat(tasks): add router policy contract'); every changed_file ⊆ :write-scope (write-scope-only); changed_files ∩ must-not-touch = ∅.")
     (:command "node scripts/check-session-trace.mjs .missiond/tasks/wave24/session-trace.lisp"
      :exit_code 0
      :ok true
      :notes "Wave24 session-trace ledger now holds 10 events: bootstrap + wave24-00 (start/commit/complete) + wave24-01 (start/commit/complete) + wave24-02 (start/commit/complete). All :seq strictly increasing; no duplicate :id; my three appends (start seq 5, commit seq 7, complete seq 8) interleaved cleanly with wave24-02's appends (start seq 6, commit seq 9, complete seq 10).")
     (:command "node scripts/check-task-memory.mjs .missiond/tasks/wave24/shared-memory.lisp"
      :exit_code 0
      :ok true
      :notes "Wave24 shared-memory ledger now holds 6 entries: bootstrap + wave24-00 claim/completion + wave24-01 claim/completion + wave24-02 claim. My two appends (claim seq 4, completion seq 6) interleaved with wave24-02's claim (seq 5).")]

  :scope_deviations []

  :trace_refs [wave24-01-trace-start-001 wave24-01-trace-commit-001 wave24-01-trace-complete-001]

  :major_decisions
    [(:decision "Use a single entry head `rule` with a structured :recommend property list rather than multi-head entries (decision/recommendation/heuristic etc.)."
      :rationale "Mirrors the session-trace-v1 pattern (single trace-event head + enumerated :kind) so analyzers and the future recommendation CLI have one shape to consume. Easier to extend with new predicate heads without churning entry shapes."
      :trace_ref "wave24-01-trace-start-001")
     (:decision "Enforce :dry-run-only and :runtime-replacement as LITERAL atom values true / false (not yes/on/off) in the checker."
      :rationale "Removes ambiguity. The data contract has one canonical surface form on disk; the checker is the single source of truth for what the cross-wave invariant looks like. Avoids accidental loosening via boolean alias drift."
      :trace_ref "wave24-01-trace-start-001")
     (:decision "Predicate heads include both `dispatch_strategy` (underscore) and `dispatch-strategy` (kebab) as aliases."
      :rationale "Underscore form matches the task contract Markdown render output and the report contract field naming; kebab form matches the task .lisp source. Accepting both lets policy authors copy-paste from either side without a transform step.")
     (:decision "Checker stays a pure validator — no scoring, no rule selection, no runtime hooks."
      :rationale "Keeps the scope tight (schema + seed + checker only). Selection / scoring lands in wave24-03's read-only recommendation CLI which will ALSO honor :dry-run-only true. No file in this commit is imported or invoked at runtime by anything in crates/**.")]

  :unexpected_work
    [(:summary "Wave24-02 agent ran in parallel and appended start/commit/complete trace events (seq 6/9/10) plus claim (seq 5) interleaved with mine. Re-read both ledgers before each append per the protocol; no overlap, no overwrite, all seq strictly increasing.")]

  :notes
    "wave24-01 implements the dry-run router policy data contract: a Lisp schema + seed policy + read-only checker. No file in this commit is consumed by runtime dispatch — wave24-03 will build the read-only recommendation CLI that ALSO honors :dry-run-only true.

Schema design points:
 - File shape: (router-policy <id> :schema :version :dry-run-only :runtime-replacement [:description] <rule>...). Header keys are tightly enumerated; unknown header keys are rejected.
 - Backend enum (5 classes, exactly per the contract requirement): claudecode, missiond-llm-router, deterministic-checker, patch-worker, verifier-worker. The `BACKEND_CLASSES` Set in the checker is the single source of truth — exporting it as a named ESM export means future tooling can import the canonical set instead of re-spelling it.
 - Predicate heads (8 total, with two aliases): kind, dispatch_strategy, dispatch-strategy, owner, status, path-glob, any, all. The `any` and `all` composites recursively validate their nested clauses.
 - Rule shape: required :id :priority :when :recommend :non-goals; optional :notes. :id and :priority are unique-within-file. :recommend is a property list (:backend <enum> :reasoning <string>); both fields are required and the backend is enum-checked.
 - :non-goals is intentionally non-empty: the contract requires every rule to list at least one explicit non-goal so reviewers can audit drift toward live dispatch.

Cross-wave invariant enforcement (the load-bearing piece):
 - The checker REJECTS any policy missing :dry-run-only true OR claiming :runtime-replacement true. Both are tested by dedicated dry-fixture cases (`runtime-replacement true REJECTED (cross-wave invariant)`, `:dry-run-only false rejected`, `missing :dry-run-only`, `missing :runtime-replacement header`).
 - The literal-atom requirement (`true` / `false`, not yes/on/off) means the data contract has one canonical on-disk form. The error message explicitly cites the cross-wave invariant: 'Router policy is advisory only — runtime dispatch is not driven by this file.'

Seed policy (.missiond/router/router-policy-v1.lisp):
 - 3 illustrative rules covering 3 backend classes: docs→claudecode, deterministic-checker for checker scripts, post-commit verifier for review/smoke kinds.
 - Each rule lists :non-goals starting with 'does not replace runtime dispatch' as a uniform reminder; some rules add scope-specific non-goals (e.g. r-deterministic-checker-tasks notes it does not authorize the deterministic-checker backend to write to crates/**).

Checker (scripts/check-router-policy.mjs):
 - Modeled on scripts/check-session-trace.mjs (most recent checker pattern). Reuses parseLisp, head, isList, keywordPropText, nodeText, nodeToStringArray, parseLisp, readKeywordProps, readLispFile from scripts/lib/missiond_lisp.mjs — no helpers were added or modified there (parallel agent wave24-02 may extend that file; my checker took the file as-is).
 - 19 dry-fixture cases across 13 categories. JSON output shape: { ok, files, policies, rules_validated, errors[] }. The cross-wave invariant rejection is exercised by both the 'runtime-replacement true REJECTED' and the ':dry-run-only false rejected' fixtures.
 - CLI: `node scripts/check-router-policy.mjs [--json] [--dry-fixture] <policy.lisp...>`. Single-file mode resolves paths against cwd, dedupes the input set, and prints either a one-line OK summary or per-error diagnostics in the standard `<file>:<line>:<col>: <severity>: <message>` format. Exit codes: 0 OK, 1 validation failure, 2 usage error.
 - Read-only by construction: the checker invokes only Node fs reads via readLispFile; no file writes, no process spawns, no network.

Wave 24 protocol followed:
 - Claim entry (wave24-01-claim-001, seq 4) appended to .missiond/tasks/wave24/shared-memory.lisp BEFORE staging.
 - This task is :session-trace-writable true — appended start (seq 5), commit (seq 7, with :commit_hash 988f7d88b467...), and complete (seq 8) events to .missiond/tasks/wave24/session-trace.lisp. Re-read the ledger tail before each append to coordinate with wave24-02 which was running in parallel.
 - Pre-commit gate: scripts/task-scope-guard.mjs --mode staged reported 3 staged files, all inside :write-scope, zero touching :must-not-touch. MISSIOND_TASK_CONTRACT env var set on the git commit invocation so the shared .githooks/pre-commit hook re-ran the same guard. Hook output appears as the leading 'task-scope-guard staged OK: wave24-01-router-policy-schema-v1 (3 staged file(s))' line in the commit output.
 - Preflight: scripts/check-missiond-hooks.mjs --json reported aligned (core.hooksPath==.githooks already set from prior wave); no install needed.
 - Post-commit verify: scripts/verify-task-contract.mjs cross-checked commit 988f7d88b467 against the contract — message subject, changed-file scope, must-not-touch intersection all green.

Constraints honored: NO Rust / SQL / cargo edits. Did not touch crates/**, .missiond/v2/**, .missiond/tasks/wave23/**, other wave24-*.lisp contracts, or .missiond/claudecode/**. Did not modify scripts/lib/missiond_lisp.mjs (out of scope and parallel agent ownership). Used Edit (not Write) on both wave24 ledger files; all three new files (schema + seed policy + checker) created via Write since they did not previously exist. Did not git add . / git stash / git reset / git checkout / push / amend / --no-verify / --force. The seed policy directory .missiond/router/ was created via mkdir -p (parent only) before writing the seed policy.")
