;; Wave 21 / Task 01 — MissionD hooksPath installer v1.
;; Schema: missiond.report-contract.v1
;; Source: .missiond/tasks/wave21/wave21-01-hooks-path-installer-v1.lisp

(report wave21-01-hooks-path-installer-v1
  :schema "missiond.report-contract.v1"
  :task_id "wave21-01-hooks-path-installer-v1"
  :status done
  :commit_hash "44c74df40182"
  :files_changed
    [".githooks/pre-commit"
     ".missiond/tasks/schema/task-contract-v1.lisp"
     "scripts/check-missiond-hooks.mjs"
     "scripts/install-missiond-hooks.mjs"]

  :acceptance_results
    [(:command "node scripts/install-missiond-hooks.mjs --dry-fixture"
      :exit_code 0
      :ok true
      :notes "install-missiond-hooks fixtures OK (8/8) — covers inspect (matches/unset/wrong value), install state machine (drift -> set, already-aligned -> no-op, unset -> set), adapter --local enforcement check, and repo-ships-pre-commit sanity. No git invoked, no disk writes.")
     (:command "node scripts/install-missiond-hooks.mjs --check --json"
      :exit_code 0
      :ok true
      :notes "Doctor JSON output: ok=false (this clone has not opted in yet: current_hooks_path=.git/hooks, expected=.githooks); hook_file_exists=true; hook_file_executable=true; advice line points at install-missiond-hooks.mjs --install. Default exit 0 because --strict was not passed; --strict was tested separately and exits 1 on drift.")
     (:command "node scripts/check-task-contract.mjs --all"
      :exit_code 0
      :ok true
      :notes "task-contract check OK (37 tasks) — schema additions (:hooks-installer / :hooks-doctor / hooks-installer-contract block) parse cleanly and do not break any existing wave14..21 task contracts.")
     (:command "node scripts/check-architecture-lisp.mjs --all-v2"
      :exit_code 0
      :ok true
      :notes "architecture-lisp check OK (20 files) — schema edit kept inside .missiond/tasks/schema/task-contract-v1.lisp; no .missiond/v2/*.lisp file was touched.")
     (:command "git diff --check -- scripts/install-missiond-hooks.mjs scripts/check-missiond-hooks.mjs .githooks/pre-commit .missiond/tasks/schema/task-contract-v1.lisp"
      :exit_code 0
      :ok true
      :notes "no whitespace errors on any of the 4 scoped paths.")
     (:command "node scripts/task-scope-guard.mjs --task .missiond/tasks/wave21/wave21-01-hooks-path-installer-v1.lisp --mode staged"
      :exit_code 0
      :ok true
      :notes "task-scope-guard staged OK (4 staged file(s)) — the 4 scoped paths inside :write-scope; zero matches against :must-not-touch (crates/** .missiond/v2/*.lisp .missiond/tasks/wave20/** .missiond/claudecode/wave20-*.md). scripts/verify-task-contract.mjs and scripts/verify-task-run.mjs (wave21-02 territory) were NOT staged even though present in the working tree.")
     (:command "node scripts/verify-task-contract.mjs .missiond/tasks/wave21/wave21-01-hooks-path-installer-v1.lisp"
      :exit_code 0
      :ok true
      :notes "task-contract verify OK against 44c74df40182 — commit message matches contract :commit :message; changed_files ⊆ :write-scope (write-scope-only); changed_files ∩ :must-not-touch = empty; acceptance commands present in contract.")]

  :scope_deviations []

  :notes
    "wave21-01 lifts repo-local pre-commit enablement out of tribal knowledge into an explicit installer + doctor pair.

     Commit 44c74df40182 (4 files / +593 / -5):
     - scripts/install-missiond-hooks.mjs (NEW, 376 lines, +x): --check / --install / --json / --dry-fixture / --strict.
       --check is read-only (default exit 0; --strict makes drift a hard non-zero exit).
       --install runs exactly one mutation: `git config --local core.hooksPath .githooks`. No-op + exit 0 when already aligned.
         Repo-local only: --local explicitly passed; never --global / --system. Adapter is injectable, fixtures cover full state machine without invoking git.
       --dry-fixture: 8 self-contained fixtures (inspect ok / unset / wrong, install drift->set / already-aligned->noop / unset->set, adapter --local enforcement, repo ships .githooks/pre-commit). No git invoked, no disk writes.
     - scripts/check-missiond-hooks.mjs (NEW, 78 lines, +x): doctor alias that delegates to install-missiond-hooks.mjs --check.
       Forwards --json and --strict; explicitly REJECTS --install / --dry-fixture / --check (predictable read-only surface for agents).
     - .githooks/pre-commit (UPDATED): comment block now documents both `node scripts/install-missiond-hooks.mjs --install` and the equivalent raw `git config core.hooksPath .githooks`; example MISSIOND_TASK_CONTRACT path bumped to a wave21 task. Behaviour unchanged: still env-gated by MISSIOND_TASK_CONTRACT, still delegates to scripts/task-scope-guard.mjs --mode staged, still read-only.
     - .missiond/tasks/schema/task-contract-v1.lisp (UPDATED): added :hooks-installer / :hooks-doctor pointers in the schema header; added a (hooks-installer-contract ...) block recording purpose, scripts, flags, modes, doctor-alias delegation rule, scope guarantees (repo-local only, single mutation), and acceptance commands. Existing scope-guard-contract :pre-commit-hook :enable line now references the installer as the preferred enable command.

     Default-on vs opt-in: Per the task brief requirement #6 ('Do not enable hooks globally; repo-local only.') the installer is intentionally opt-in. This clone was NOT auto-enabled — the user / next agent runs `node scripts/install-missiond-hooks.mjs --install` to opt in. The doctor will report drift until then. Wave20-10's pending 'default-on' suggestion is intentionally NOT acted on here: the task contract requirements only require providing the installer/doctor, not flipping the default. A future wave can decide to invoke the installer from a bootstrap step if desired.

     Wave 21 protocol followed:
     - claim entry (wave21-01-claim-001, seq 4) appended to .missiond/tasks/wave21/shared-memory.lisp BEFORE staging.
       completion entry (wave21-01-completion-001, seq 6) appended AFTER post-commit verify. Ledger remains intentionally untracked (out-of-scope per the task contract :must-not-touch wave20 paths and per wave21 protocol).
     - Edit tool used on .missiond/tasks/wave21/shared-memory.lisp (claim + completion only — no edits to prior entries; the parallel wave21-04 session's seq=5 claim was preserved as-is). Write tool used only for the two NEW scripts and this report file.
     - Pre-commit gate: scripts/task-scope-guard.mjs --mode staged green (4 staged files all in :write-scope, zero touching :must-not-touch). MISSIOND_TASK_CONTRACT env var set on the git commit invocation so the .githooks/pre-commit hook (when enabled) re-runs the same guard.
     - Post-commit verify: scripts/verify-task-contract.mjs cross-checked commit 44c74df40182 against the contract — message + scope + must-not-touch + acceptance presence all green.

     Constraints honored: NO Rust / Cargo / SQL / .missiond/v2/* edits. Did not touch crates/**, .missiond/v2/*.lisp, .missiond/tasks/wave20/**, or .missiond/claudecode/wave20-*.md. Did NOT stage scripts/verify-task-contract.mjs or scripts/verify-task-run.mjs (wave21-02 ownership) even though they appear modified / new in the working tree from the parallel session. Did not git add . / git stash / git reset / git checkout. .githooks/pre-commit content edits stayed inside the comment block (no behaviour change, byte-for-byte identical exec path).

     Remaining untracked after this commit: .missiond/tasks/wave21/ (entire dir: contracts wave21-00..10 + shared-memory.lisp + reports/wave21-01-hooks-path-installer-v1.report.lisp written by this task) and 11 .missiond/claudecode/wave21-*.md briefs — all expected, all destined for wave22-NN's archive task next cycle. scripts/verify-task-contract.mjs (modified) and scripts/verify-task-run.mjs (new) belong to the parallel wave21-02 session and will be committed by that session.")
