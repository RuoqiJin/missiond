;; Wave 25 / Task 00 — Archive Wave 24 task artifacts.
;; Schema: missiond.report-contract.v1
;; Source: .missiond/tasks/wave25/wave25-00-archive-wave24-artifacts.lisp

(report wave25-00-archive-wave24-artifacts
  :schema "missiond.report-contract.v1"
  :task_id "wave25-00-archive-wave24-artifacts"
  :status done
  :commit_hash "83b575d9c19d"
  :files_changed
    [".missiond/claudecode/wave24-00-archive-wave23-artifacts.md"
     ".missiond/claudecode/wave24-01-router-policy-schema-v1.md"
     ".missiond/claudecode/wave24-02-trace-corpus-index-v0.md"
     ".missiond/claudecode/wave24-03-router-recommendation-cli-v0.md"
     ".missiond/claudecode/wave24-04-plan-router-dry-run-surface-v0.md"
     ".missiond/claudecode/wave24-05-renderer-router-context-v0.md"
     ".missiond/claudecode/wave24-06-router-dry-run-smoke-v0.md"
     ".missiond/claudecode/wave24-07-lisp-backfill-router-dry-run-status.md"
     ".missiond/claudecode/wave24-08-parallel-dispatch-index.md"
     ".missiond/tasks/wave24/reports/wave24-00-archive-wave23-artifacts.report.lisp"
     ".missiond/tasks/wave24/reports/wave24-01-router-policy-schema-v1.report.lisp"
     ".missiond/tasks/wave24/reports/wave24-02-trace-corpus-index-v0.report.lisp"
     ".missiond/tasks/wave24/reports/wave24-03-router-recommendation-cli-v0.report.lisp"
     ".missiond/tasks/wave24/reports/wave24-04-plan-router-dry-run-surface-v0.report.lisp"
     ".missiond/tasks/wave24/reports/wave24-05-renderer-router-context-v0.report.lisp"
     ".missiond/tasks/wave24/reports/wave24-06-router-dry-run-smoke-v0.report.lisp"
     ".missiond/tasks/wave24/session-trace.lisp"
     ".missiond/tasks/wave24/shared-memory.lisp"
     ".missiond/tasks/wave24/wave24-00-archive-wave23-artifacts.lisp"
     ".missiond/tasks/wave24/wave24-01-router-policy-schema-v1.lisp"
     ".missiond/tasks/wave24/wave24-02-trace-corpus-index-v0.lisp"
     ".missiond/tasks/wave24/wave24-03-router-recommendation-cli-v0.lisp"
     ".missiond/tasks/wave24/wave24-04-plan-router-dry-run-surface-v0.lisp"
     ".missiond/tasks/wave24/wave24-05-renderer-router-context-v0.lisp"
     ".missiond/tasks/wave24/wave24-06-router-dry-run-smoke-v0.lisp"
     ".missiond/tasks/wave24/wave24-07-lisp-backfill-router-dry-run-status.lisp"
     ".missiond/tasks/wave24/wave24-08-parallel-dispatch-index.lisp"]

  :acceptance_results
    [(:command "node scripts/check-task-contract.mjs --all"
      :exit_code 0
      :ok true
      :notes "task-contract check OK (74 tasks) — all wave22 + wave23 + wave24 + wave25 task contracts parse and pass shape / scope / must-not-touch / acceptance / commit-policy validation. Total includes the 9 wave24-NN contracts being archived this commit plus the wave25 dispatch set already on disk.")
     (:command "node scripts/check-task-memory.mjs .missiond/tasks/wave24/shared-memory.lisp"
      :exit_code 0
      :ok true
      :notes "shared-memory check OK (1 ledger, 15 entries) — wave24 ledger committed verbatim; append-only invariant + bootstrap entry + 14 prior wave24 task entries (claims/observations/completions from wave24-00..06) preserved.")
     (:command "node scripts/check-session-trace.mjs .missiond/tasks/wave24/session-trace.lisp"
      :exit_code 0
      :ok true
      :notes "session-trace check OK (1 trace, 22 events) — wave24 trace ledger archived verbatim with all start/commit/complete events from wave24-00..06 preserved. wave24-07 (Codex Lisp backfill) and wave24-08 (coordination index) emit no trace events.")
     (:command "git diff --cached --name-only"
      :exit_code 0
      :ok true
      :notes "27 staged paths printed before commit, all inside :write-scope: 9 wave24 task contracts (00..08) + 9 rendered briefs + 7 wave24 reports (00..06) + shared-memory.lisp + session-trace.lisp.")
     (:command "git diff --check -- .missiond/tasks/wave24 .missiond/claudecode"
      :exit_code 0
      :ok true
      :notes "no whitespace errors on any of the archived wave24 paths or surrounding .missiond/claudecode entries; no edits made to wave24 file content during archival (Edit tool not used on wave24 files).")
     (:command "node scripts/task-scope-guard.mjs --task .missiond/tasks/wave25/wave25-00-archive-wave24-artifacts.lisp --mode staged"
      :exit_code 0
      :ok true
      :notes "task-scope-guard staged OK: wave25-00-archive-wave24-artifacts (27 staged file(s)) — all 27 staged paths inside :write-scope (.missiond/tasks/wave24/** + .missiond/claudecode/wave24-*.md); zero matches against :must-not-touch (crates/** scripts/** .missiond/v2/** .missiond/tasks/wave25/wave25-*.lisp .missiond/claudecode/wave25-*.md).")
     (:command "node scripts/verify-task-contract.mjs .missiond/tasks/wave25/wave25-00-archive-wave24-artifacts.lisp"
      :exit_code 0
      :ok true
      :notes "task-contract verify OK: wave25-00-archive-wave24-artifacts against 83b575d9c19d — commit hash exists; commit message contains chore(wave24) prefix per contract; changed_files ⊆ write-scope (write-scope-only); changed_files ∩ must-not-touch = ∅; acceptance commands present in contract.")
     (:command "node scripts/check-missiond-hooks.mjs --json"
      :exit_code 0
      :ok true
      :notes "preflight OK; core.hooksPath aligned to .githooks; .githooks/pre-commit exists and is executable; no install required.")]

  :scope_deviations []

  :trace_refs [wave25-00-trace-start-001 wave25-00-trace-commit-001 wave25-00-trace-complete-001]

  :unexpected_work
    [(:summary "wave24 reports 07/08 do not exist on disk — those wave24 tasks did not emit machine-readable reports during the wave24 cycle. wave24-07 is a Codex-owned Lisp backfill committed earlier as bce64e2 (its acceptance is the in-tree v2 status update, not a separate report file). wave24-08 is a coordination dispatch index whose contract explicitly states 'No report required; this is a coordination index.' So 7 reports archived for 9 task contracts — same convention wave22-09 / wave23-07/08/09 followed.")]

  :notes
    "wave25-00 archives the 27 untracked Wave 24 task artifacts left after the Wave 24 implementation cycle into a single chore(wave24) commit (83b575d9c19d, 27 files / +2912 insertions, zero deletions). Breakdown:
     - 9 task contracts (.missiond/tasks/wave24/wave24-{00..08}-*.lisp).
     - 9 rendered briefs (.missiond/claudecode/wave24-{00..08}-*.md).
     - 7 machine-readable reports (.missiond/tasks/wave24/reports/wave24-00..06-*.report.lisp). wave24-07/08 produced no machine reports — wave24-07 was Codex Lisp backfill (commit bce64e2) and wave24-08 is a coordination dispatch index that declares no report.
     - 1 shared-memory ledger (.missiond/tasks/wave24/shared-memory.lisp) with 15 entries: 1 bootstrap + 14 wave24 task claims / observations / completions from wave24-00..06.
     - 1 session-trace ledger (.missiond/tasks/wave24/session-trace.lisp) with 22 events covering wave24-00..06 start/commit/complete cycles.
     Wave 25 protocol followed:
     - Claim entry (wave25-00-claim-001) appended to .missiond/tasks/wave25/shared-memory.lisp BEFORE staging (seq 2).
     - This task is :session-trace-writable true — appended start (seq 2), commit (seq 3, with :commit_hash 83b575d9c19d), and complete (seq 4) events to .missiond/tasks/wave25/session-trace.lisp. wave25 ledger now holds 4 events; will itself be archived in wave26.
     - Edit tool used on wave25 ledger files only; Write tool used only for this report file (all three remain intentionally untracked — they are out-of-scope for this commit per :must-not-touch .missiond/tasks/wave25/wave25-*.lisp + .missiond/claudecode/wave25-*.md, and follow the same append-out-of-scope pattern wave24-00 used during its cycle).
     - Pre-commit gate: scripts/task-scope-guard.mjs --mode staged reported 27 staged files, all inside :write-scope, zero touching :must-not-touch. MISSIOND_TASK_CONTRACT env var set on the git commit invocation so the shared .githooks/pre-commit hook re-runs the same guard. Hook output confirmed the staged-file count match.
     - Preflight: scripts/check-missiond-hooks.mjs --json reported aligned (core.hooksPath==.githooks already set from prior wave); no install needed.
     - Post-commit verify: scripts/verify-task-contract.mjs cross-checked commit 83b575d9c19d against the contract — message prefix, changed-file scope, must-not-touch intersection, acceptance command presence all green.
     - Completion entry (wave25-00-completion-001) appended to .missiond/tasks/wave25/shared-memory.lisp after verify (seq 3).
     Constraints honored: NO Rust / SQL / JS / Cargo edits. Did not touch crates/** or scripts/**. Did not touch .missiond/v2/**. Did not stage any wave25 contract, brief, ledger, or report (.missiond/tasks/wave25/wave25-*.lisp + .missiond/claudecode/wave25-*.md all remain untracked, ready for wave26's own archive task next cycle). Did not git add . / git stash / git reset / git checkout. Used Edit (not Write) on both wave25 ledger files; the wave24 file content was committed verbatim with no edits required (git diff --check clean).
     Remaining untracked after this commit: .missiond/tasks/wave25/ (contracts wave25-NN + this report + the appended-to shared-memory + session-trace) and the wave25 .missiond/claudecode/wave25-*.md briefs — all expected, all destined for wave26-NN's own archive task next cycle.")
