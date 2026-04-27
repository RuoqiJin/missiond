;; Wave 21 / Task 02 — Task run verifier v1.
;; Schema: missiond.report-contract.v1
;; Source: .missiond/tasks/wave21/wave21-02-run-verifier-v1.lisp

(report wave21-02-run-verifier-v1
  :schema "missiond.report-contract.v1"
  :task_id "wave21-02-run-verifier-v1"
  :status done
  :commit_hash "1335fa7fbe85"
  :files_changed
    ["scripts/verify-task-run.mjs"
     "scripts/verify-task-contract.mjs"
     ".missiond/tasks/schema/report-contract-v1.lisp"
     ".missiond/tasks/schema/shared-memory-v1.lisp"]

  :acceptance_results
    [(:command "node scripts/verify-task-run.mjs --dry-fixture"
      :exit_code 0
      :ok true
      :notes "task-run verify fixtures OK (12 fixtures, 7 helper cases). Fixtures cover: all-green; report :task_id mismatch; report :commit_hash mismatch; full-sha match; commit message mismatch (delegated); file outside :write-scope (delegated); commit hits :must-not-touch (delegated); shared-memory missing completion entry; shared-memory has completion for a different task; --allow-missing-memory skip path; missing memory without flag hard-fails; report :status != done emits warning but is non-fatal. Helper cases cover commitHashMatches: short prefix (>=7 hex), full sha, case-insensitivity, too-short prefix, empty, non-hex chars, non-prefix.")
     (:command "node scripts/check-task-contract.mjs --all"
      :exit_code 0
      :ok true
      :notes "task-contract check OK (37 tasks). All wave21 task contracts (00..10) plus prior-wave contracts continue to pass shape / scope / must-not-touch / acceptance / commit-policy validation; no regression introduced by gating verify-task-contract.mjs main() or by the new run-verifier.")
     (:command "node scripts/check-task-report.mjs --dry-fixture"
      :exit_code 0
      :ok true
      :notes "task-report fixtures OK (10). Existing report-contract checker untouched; cross-link in .missiond/tasks/schema/report-contract-v1.lisp adding :run-verifier did not change validation behaviour.")
     (:command "node scripts/check-task-memory.mjs --dry-fixture"
      :exit_code 0
      :ok true
      :notes "shared-memory fixtures OK (13). Existing memory checker untouched; cross-link in .missiond/tasks/schema/shared-memory-v1.lisp adding :run-verifier did not change validation behaviour.")
     (:command "node scripts/check-architecture-lisp.mjs --all-v2"
      :exit_code 0
      :ok true
      :notes "architecture-lisp check OK (20 files). Schema edits stay structurally valid as architecture lisp.")
     (:command "git diff --check -- scripts/verify-task-run.mjs scripts/verify-task-contract.mjs scripts/check-task-report.mjs scripts/check-task-memory.mjs scripts/lib/missiond_lisp.mjs .missiond/tasks/schema/report-contract-v1.lisp .missiond/tasks/schema/shared-memory-v1.lisp"
      :exit_code 0
      :ok true
      :notes "no whitespace errors on any of the 7 declared write-scope files. Only 4 of 7 actually changed (verify-task-run.mjs new; verify-task-contract.mjs main() gated; report-contract-v1.lisp + shared-memory-v1.lisp cross-link the run-verifier). check-task-report.mjs, check-task-memory.mjs, scripts/lib/missiond_lisp.mjs untouched (write-scope is permissive, not mandatory).")
     (:command "node scripts/task-scope-guard.mjs --task .missiond/tasks/wave21/wave21-02-run-verifier-v1.lisp --mode staged"
      :exit_code 0
      :ok true
      :notes "task-scope-guard staged OK: wave21-02-run-verifier-v1 (4 staged file(s)). All 4 staged paths in :write-scope; zero matches against :must-not-touch (crates/**, .missiond/v2/*.lisp, .githooks/pre-commit, scripts/install-missiond-hooks.mjs/check-missiond-hooks.mjs, scripts/task-scope-guard.mjs/.missiond/tasks/schema/task-contract-v1.lisp). Pre-commit gate ran cleanly before commit.")
     (:command "node scripts/verify-task-contract.mjs .missiond/tasks/wave21/wave21-02-run-verifier-v1.lisp"
      :exit_code 0
      :ok true
      :notes "task-contract verify OK: wave21-02-run-verifier-v1 against 1335fa7fbe85. Post-commit verifier confirms: commit message subject equals contract :commit :message (\"feat(tasks): verify complete task runs\"); changed files (4) ⊆ :write-scope; changed files ∩ :must-not-touch = ∅; acceptance commands present in contract.")
     (:command "node scripts/verify-task-run.mjs --task .missiond/tasks/wave21/wave21-02-run-verifier-v1.lisp --report .missiond/tasks/wave21/reports/wave21-02-run-verifier-v1.report.lisp --memory .missiond/tasks/wave21/shared-memory.lisp --commit 1335fa7"
      :exit_code 0
      :ok true
      :notes "Dogfood: the run-verifier verifies its own task run end-to-end. ok=true. checks.task_contract.ok=true (delegated); checks.report_task_id.ok=true (expected/got both wave21-02-run-verifier-v1); checks.report_commit_hash.ok=true (got 1335fa7fbe85, expected_full 1335fa7fbe85dede5d5dd391bf2a99eff5b8fab2 — short-prefix match, >=7 hex); checks.shared_memory_completion.ok=true (matched_entry_id wave21-02-completion-001 in .missiond/tasks/wave21/shared-memory.lisp). Zero errors, zero warnings.")]

  :scope_deviations []

  :notes
    "wave21-02 ships scripts/verify-task-run.mjs as the three-in-one post-run proof for MissionD task runs. CLI synopsis: --task <task.lisp> --report <report.lisp> --memory <shared-memory.lisp> --commit <hash> [--json] [--allow-missing-memory], plus --dry-fixture for self-contained fixtures (no git, no I/O). Architecture: imports loadContract, readCommit, verifyContract from scripts/verify-task-contract.mjs (whose main() is now gated by import.meta.url so an import does not trigger CLI parsing); parses report and shared-memory ledger directly via the shared scripts/lib/missiond_lisp.mjs Lisp reader (no need to depend on check-task-report.mjs or check-task-memory.mjs runtime). Verification flow per run: (1) verify-task-contract delegated check — commit subject vs :commit :message, changed files ⊆ :write-scope when scope-check=write-scope-only, changed files ∩ :must-not-touch = ∅; (2) report :task_id == contract id; (3) report :commit_hash matches resolved git sha (full equality OR repo-style short prefix >=7 hex chars, case-insensitive); (4) shared-memory has a (completion :task <contract-id>) entry (lookup by :task; matched entry id surfaced in JSON output for audit). Missing-memory contract: --memory is required by default; passing --memory with a non-existent path or omitting --memory hard-fails with a structured diagnostic that names the missing file; --allow-missing-memory turns the failure into a warning that explicitly says 'task run is NOT fully proven without a completion entry', so silent skip is impossible. Read-only invariant: assertReadOnlyGit (exported) documents the 14 forbidden git verbs (add/commit/reset/checkout/restore/switch/stash/push/merge/rebase/rm/mv/tag/cherry-pick/revert/clean/fetch/pull); the only git access reachable from this script is verify-task-contract.readCommit's `git rev-parse --verify | git log -1 --format=%B | git show --no-renames --name-only`, all read-only. Dry-fixture coverage: 12 named fixtures + 7 commitHashMatches helper cases; covers every error path including delegated task-contract failures, every cross-check, both missing-memory branches, and the non-fatal status warning. Wave 21 protocol followed: claim entry (wave21-02-claim-001, seq 7) appended to .missiond/tasks/wave21/shared-memory.lisp BEFORE staging (parallel-aware seq numbering — wave21-01 already used seq 4 + 6); ledger remains intentionally untracked (out-of-scope per :must-not-touch .missiond/tasks/wave21/**); pre-commit task-scope-guard --mode staged green (4 files, all in :write-scope); MISSIOND_TASK_CONTRACT env var set on git commit (the shared .githooks/pre-commit hook re-runs the same guard when enabled per clone); post-commit scripts/verify-task-contract.mjs green; completion entry (wave21-02-completion-001, seq 8) appended after commit; dogfood self-verify scripts/verify-task-run.mjs against this very task run returns ok=true with all 4 checks green. Constraints honored: NO Rust / SQL / Cargo edits; did not touch crates/** or .missiond/v2/*.lisp or .githooks/pre-commit or scripts/install-missiond-hooks.mjs / check-missiond-hooks.mjs / task-scope-guard.mjs / .missiond/tasks/schema/task-contract-v1.lisp (wave21-01 scope) or .missiond/tasks/wave20/**; no git add . / git stash / git reset / git checkout / git push --force / git amend; never modified prior shared-memory entries. Edit tool used on verify-task-contract.mjs (added pathToFileURL import + import.meta.url gate around main()), shared-memory.lisp (claim + completion appended), and both schema lisp files (status string + :run-verifier cross-link); Write tool used only for verify-task-run.mjs (new file) and this report (out-of-scope per :must-not-touch .missiond/tasks/wave21/** and intentional). Remaining untracked after this commit: .missiond/tasks/wave21/shared-memory.lisp (with new claim+completion), .missiond/tasks/wave21/reports/wave21-02-run-verifier-v1.report.lisp, the 11 wave21-NN .md briefs, and 9 wave21 contract files — all expected, all destined for wave21-NN's own archive task next cycle.")
