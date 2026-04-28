;; Wave 26 / Task 00 — Archive Wave 25 task artifacts.
;; Schema: missiond.report-contract.v1
;; Source: .missiond/tasks/wave26/wave26-00-archive-wave25-artifacts.lisp

(report wave26-00-archive-wave25-artifacts
  :schema "missiond.report-contract.v1"
  :task_id "wave26-00-archive-wave25-artifacts"
  :status done
  :commit_hash "72d57f83b88b"
  :files_changed
    [".missiond/claudecode/wave25-00-archive-wave24-artifacts.md"
     ".missiond/claudecode/wave25-01-router-policy-corpus-evaluator-v0.md"
     ".missiond/claudecode/wave25-02-report-router-recommendation-fields-v0.md"
     ".missiond/claudecode/wave25-03-plan-router-trace-index-confidence-v1.md"
     ".missiond/claudecode/wave25-04-renderer-router-recommendation-command-v1.md"
     ".missiond/claudecode/wave25-05-router-policy-measurement-smoke-v1.md"
     ".missiond/claudecode/wave25-06-lisp-backfill-router-measurement-status.md"
     ".missiond/claudecode/wave25-07-parallel-dispatch-index.md"
     ".missiond/tasks/wave25/reports/wave25-00-archive-wave24-artifacts.report.lisp"
     ".missiond/tasks/wave25/reports/wave25-01-router-policy-corpus-evaluator-v0.report.lisp"
     ".missiond/tasks/wave25/reports/wave25-02-report-router-recommendation-fields-v0.report.lisp"
     ".missiond/tasks/wave25/reports/wave25-03-plan-router-trace-index-confidence-v1.report.lisp"
     ".missiond/tasks/wave25/reports/wave25-04-renderer-router-recommendation-command-v1.report.lisp"
     ".missiond/tasks/wave25/reports/wave25-05-router-policy-measurement-smoke-v1.report.lisp"
     ".missiond/tasks/wave25/session-trace.lisp"
     ".missiond/tasks/wave25/shared-memory.lisp"
     ".missiond/tasks/wave25/wave25-00-archive-wave24-artifacts.lisp"
     ".missiond/tasks/wave25/wave25-01-router-policy-corpus-evaluator-v0.lisp"
     ".missiond/tasks/wave25/wave25-02-report-router-recommendation-fields-v0.lisp"
     ".missiond/tasks/wave25/wave25-03-plan-router-trace-index-confidence-v1.lisp"
     ".missiond/tasks/wave25/wave25-04-renderer-router-recommendation-command-v1.lisp"
     ".missiond/tasks/wave25/wave25-05-router-policy-measurement-smoke-v1.lisp"
     ".missiond/tasks/wave25/wave25-06-lisp-backfill-router-measurement-status.lisp"
     ".missiond/tasks/wave25/wave25-07-parallel-dispatch-index.lisp"]

  :acceptance_results
    [(:command "node scripts/check-task-contract.mjs --all"
      :exit_code 0
      :ok true
      :notes "task-contract check OK (83 tasks) — all wave22 + wave23 + wave24 + wave25 + wave26 task contracts parse and pass shape / scope / must-not-touch / acceptance / commit-policy validation. Total includes the 8 wave25-NN contracts being archived this commit plus the wave26 dispatch set already on disk.")
     (:command "node scripts/check-task-memory.mjs .missiond/tasks/wave25/shared-memory.lisp"
      :exit_code 0
      :ok true
      :notes "shared-memory check OK (1 ledger, 13 entries) — wave25 ledger committed verbatim; append-only invariant + bootstrap entry + 12 prior wave25 task entries (claims/observations/completions from wave25-00..05) preserved.")
     (:command "node scripts/check-session-trace.mjs .missiond/tasks/wave25/session-trace.lisp"
      :exit_code 0
      :ok true
      :notes "session-trace check OK (1 trace, 19 events) — wave25 trace ledger archived verbatim with all start/commit/complete events from wave25-00..05 preserved. wave25-06 (Codex Lisp backfill) and wave25-07 (coordination index) emit no trace events.")
     (:command "git diff --cached --name-only"
      :exit_code 0
      :ok true
      :notes "24 staged paths printed before commit, all inside :write-scope: 8 wave25 task contracts (00..07) + 8 rendered briefs + 6 wave25 reports (00..05) + shared-memory.lisp + session-trace.lisp.")
     (:command "git diff --check -- .missiond/tasks/wave25 .missiond/claudecode"
      :exit_code 0
      :ok true
      :notes "no whitespace errors on any of the archived wave25 paths or surrounding .missiond/claudecode entries; no edits made to wave25 file content during archival (Edit tool not used on wave25 files).")
     (:command "node scripts/task-scope-guard.mjs --task .missiond/tasks/wave26/wave26-00-archive-wave25-artifacts.lisp --mode staged"
      :exit_code 0
      :ok true
      :notes "task-scope-guard staged OK: wave26-00-archive-wave25-artifacts (24 staged file(s)) — all 24 staged paths inside :write-scope (.missiond/tasks/wave25/** + .missiond/claudecode/wave25-*.md); zero matches against :must-not-touch (crates/** scripts/** .missiond/v2/** .missiond/tasks/wave26/wave26-*.lisp .missiond/claudecode/wave26-*.md).")
     (:command "node scripts/verify-task-contract.mjs .missiond/tasks/wave26/wave26-00-archive-wave25-artifacts.lisp"
      :exit_code 0
      :ok true
      :notes "task-contract verify OK: wave26-00-archive-wave25-artifacts against 72d57f83b88b — commit hash exists; commit message contains chore(wave25) prefix per contract; changed_files ⊆ write-scope (write-scope-only); changed_files ∩ must-not-touch = ∅; acceptance commands present in contract.")
     (:command "node scripts/check-missiond-hooks.mjs --json"
      :exit_code 0
      :ok true
      :notes "preflight OK; core.hooksPath aligned to .githooks; .githooks/pre-commit exists and is executable; no install required.")]

  :scope_deviations []

  :trace_refs [wave26-00-trace-start-001 wave26-00-trace-commit-001 wave26-00-trace-complete-001]

  :unexpected_work
    [(:summary "wave25 reports 06/07 do not exist on disk — those wave25 tasks did not emit machine-readable reports during the wave25 cycle. wave25-06 is a Codex-owned Lisp backfill committed earlier as 3310b22 (its acceptance is the in-tree v2 status update, not a separate report file). wave25-07 is a coordination dispatch index whose contract explicitly states no report is required. So 6 reports archived for 8 task contracts — same convention wave22-09 / wave23-07/08/09 / wave24-07/08 followed.")]

  :notes
    "wave26-00 archives the 24 untracked Wave 25 task artifacts left after the Wave 25 implementation cycle into a single chore(wave25) commit (72d57f83b88b, 24 files / +2813 insertions, zero deletions). Breakdown:
     - 8 task contracts (.missiond/tasks/wave25/wave25-{00..07}-*.lisp).
     - 8 rendered briefs (.missiond/claudecode/wave25-{00..07}-*.md).
     - 6 machine-readable reports (.missiond/tasks/wave25/reports/wave25-00..05-*.report.lisp). wave25-06/07 produced no machine reports — wave25-06 was Codex Lisp backfill (commit 3310b22) and wave25-07 is a coordination dispatch index that declares no report.
     - 1 shared-memory ledger (.missiond/tasks/wave25/shared-memory.lisp) with 13 entries: 1 bootstrap + 12 wave25 task claims / completions from wave25-00..05.
     - 1 session-trace ledger (.missiond/tasks/wave25/session-trace.lisp) with 19 events covering wave25-00..05 start/commit/complete cycles.
     Wave 26 protocol followed:
     - Claim entry (wave26-00-claim-001) appended to .missiond/tasks/wave26/shared-memory.lisp BEFORE staging (seq 2).
     - This task is :session-trace-writable true — appended start (seq 2), commit (seq 3, with :commit_hash 72d57f83b88b), and complete (seq 4) events to .missiond/tasks/wave26/session-trace.lisp. wave26 ledger now holds 4 events; will itself be archived in wave27.
     - Edit tool used on wave26 ledger files only; Write tool used only for this report file (all three remain intentionally untracked — they are out-of-scope for this commit per :must-not-touch .missiond/tasks/wave26/wave26-*.lisp + .missiond/claudecode/wave26-*.md, and follow the same append-out-of-scope pattern wave25-00 used during its cycle).
     - Pre-commit gate: scripts/task-scope-guard.mjs --mode staged reported 24 staged files, all inside :write-scope, zero touching :must-not-touch. MISSIOND_TASK_CONTRACT env var set on the git commit invocation so the shared .githooks/pre-commit hook re-runs the same guard. Hook output confirmed the staged-file count match.
     - Preflight: scripts/check-missiond-hooks.mjs --json reported aligned (core.hooksPath==.githooks already set from prior wave); no install needed.
     - Post-commit verify: scripts/verify-task-contract.mjs cross-checked commit 72d57f83b88b against the contract — message prefix, changed-file scope, must-not-touch intersection, acceptance command presence all green.
     - Completion entry (wave26-00-completion-001) appended to .missiond/tasks/wave26/shared-memory.lisp after verify (seq 3).
     Constraints honored: NO Rust / SQL / JS / Cargo edits. Did not touch crates/** or scripts/**. Did not touch .missiond/v2/**. Did not stage any wave26 contract, brief, ledger, or report (.missiond/tasks/wave26/wave26-*.lisp + .missiond/claudecode/wave26-*.md all remain untracked, ready for wave27's own archive task next cycle). Did not git add . / git stash / git reset / git checkout. Used Edit (not Write) on both wave26 ledger files; the wave25 file content was committed verbatim with no edits required (git diff --check clean).
     Remaining untracked after this commit: .missiond/tasks/wave26/ (contracts wave26-NN + this report + the appended-to shared-memory + session-trace) and the wave26 .missiond/claudecode/wave26-*.md briefs — all expected, all destined for wave27-NN's own archive task next cycle.")
