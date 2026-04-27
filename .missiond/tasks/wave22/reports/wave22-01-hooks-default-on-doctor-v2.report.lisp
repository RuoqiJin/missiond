;; Wave 22 / Task 01 — Hooks default-on doctor v2.
;; Schema: missiond.report-contract.v1
;; Source: .missiond/tasks/wave22/wave22-01-hooks-default-on-doctor-v2.lisp

(report wave22-01-hooks-default-on-doctor-v2
  :schema "missiond.report-contract.v1"
  :task_id "wave22-01-hooks-default-on-doctor-v2"
  :status done
  :commit_hash "49555c41814b"
  :files_changed
    [".missiond/tasks/schema/task-contract-v1.lisp"
     "scripts/check-missiond-hooks.mjs"
     "scripts/install-missiond-hooks.mjs"
     "scripts/render-claudecode-task.mjs"]

  :acceptance_results
    [(:command "node scripts/install-missiond-hooks.mjs --dry-fixture"
      :exit_code 0
      :ok true
      :notes "install-missiond-hooks fixtures OK (11/11) — doctor states: installed, unset, wrong-path, missing-hook-file. Fixtures grew from 8 to 11: 4 doctor-state fixtures (installed/unset/wrong-path/missing-hook-file) + 3 install state-machine fixtures (drift->aligned / no-op / unset->aligned) + install-refuses-when-hook-file-missing + adapter --local enforcement + adviceFor() install-command surface + repo ships .githooks/pre-commit sanity.")
     (:command "node scripts/check-missiond-hooks.mjs --json"
      :exit_code 0
      :ok true
      :notes "check-missiond-hooks --json delegates to install-missiond-hooks --check and prints v2 payload: mode=check ok=false severity=preflight-drift preflight=core.hooksPath reason=hooks-path-wrong (this clone has core.hooksPath=/Users/jinchen/Projects/missiond/.git/hooks, expected .githooks) advice='core.hooksPath is set to ..., expected .githooks. Run `node scripts/install-missiond-hooks.mjs --install` ...' install_command='node scripts/install-missiond-hooks.mjs --install'. Exit 0 (drift is not strict by default, per v2 default-on doctor design).")
     (:command "node scripts/check-task-contract.mjs --all"
      :exit_code 0
      :ok true
      :notes "task-contract check OK (47 tasks). Schema task-contract-v1.lisp updates (status note bump + renderer-contract :machine-context-rendered + :backward-compatibility entries for hooks-doctor preflight block + hooks-installer-contract v2 expansion with :default-mode :doctor-states :renderer-integration :acceptance) parse and validate against the schema's own shape requirements.")
     (:command "node scripts/check-architecture-lisp.mjs --all-v2"
      :exit_code 0
      :ok true
      :notes "architecture-lisp check OK (20 files). v2 architecture lisps untouched per :must-not-touch contract; this acceptance gate guards against accidental drift.")
     (:command "git diff --check -- scripts/install-missiond-hooks.mjs scripts/check-missiond-hooks.mjs scripts/render-claudecode-task.mjs .missiond/tasks/schema/task-contract-v1.lisp"
      :exit_code 0
      :ok true
      :notes "git diff --check clean across all 4 in-scope files; no whitespace errors introduced by the edits.")]

  :scope_deviations []

  :notes
    "wave22-01 promotes the wave21-01 hooks-installer to a default-on read-only doctor + renderer-surfaced preflight without ever mutating git config from the doctor or the renderer. Commit 49555c41814b (4 files, +292/-70).

Default-on doctor (install-missiond-hooks.mjs):
- Default mode: invoking the script with NO mode flag is now equivalent to --check (the read-only doctor). Previously failUsage required one of --check|--install|--dry-fixture. The doctor remains read-only and exits 0 by default; --strict turns drift into a hard non-zero exit.
- JSON payload v2 fields: :severity (ok | preflight-drift), :preflight ('core.hooksPath'), :reason (aligned | hooks-path-unset | hooks-path-wrong | hook-file-missing), :advice (concrete install command for non-ok states), :install_command ('node scripts/install-missiond-hooks.mjs --install' for non-ok, null for ok).
- DOCTOR_SEVERITY constant exported so other tooling can branch on it.
- adviceFor() differentiates per reason: hook-file-missing tells the operator to restore the file first; hooks-path-unset / hooks-path-wrong both surface the install command but with state-specific framing.
- performInstall() unchanged in surface but already refuses when the hook file is absent — exercised explicitly by fixture #8.

Renderer (scripts/render-claudecode-task.mjs):
- New renderHooksDoctorPreflight() function emits a preflight block above the existing git-add / staged-guard / git-commit fenced block in :commit :required briefs.
- The block contains 2 lines inside a fenced code block: `node scripts/check-missiond-hooks.mjs --json` (read-only doctor) and `node scripts/install-missiond-hooks.mjs --install` (only run when doctor reports drift). The renderer NEVER invokes either; it only emits the lines.
- The trailing prose paragraph below the staged-guard fence updated to reference `node scripts/install-missiond-hooks.mjs --install` instead of bare `git config core.hooksPath .githooks` (the latter still mentioned as the equivalent for users who insist on doing it themselves).
- No-commit briefs are unchanged.

Schema (.missiond/tasks/schema/task-contract-v1.lisp):
- :status note bumped to mention v2 default-on hooks doctor preflight (wave22-01).
- renderer-contract :machine-context-rendered gains a 9th entry describing the hooks-doctor preflight block.
- renderer-contract :backward-compatibility gains a 4th entry pinning the placement (above the existing fenced block) and the never-mutates-git-config invariant.
- hooks-installer-contract :purpose extended to describe v2 default-on doctor; new :default-mode field; modes now describe v2 JSON payload fields, install-refuses-on-missing-hook-file, and the default-mode rule; new :doctor-states section enumerates the 4 doctor states; :doctor-alias gains :default-on true; new :renderer-integration field documents the placement + emitted commands + mutating boundary; :scope gains 'doctor + renderer never mutate git config under any circumstance'; :acceptance updated to 11 fixtures and adds 2 new acceptance bullets (default-mode equivalence + rendered briefs include doctor preflight).

Fixtures grew from 8 to 11 with explicit doctor_states_covered = [installed unset wrong-path missing-hook-file] in the JSON summary:
  1. doctor state: installed (aligned)
  2. doctor state: unset core.hooksPath (hooks-path-unset)
  3. doctor state: wrong core.hooksPath (hooks-path-wrong)
  4. doctor state: missing hook file (hook-file-missing) — uses a fake repo root that does not exist (no disk writes)
  5. install: drift -> sets core.hooksPath to .githooks
  6. install: already aligned -> no-op
  7. install: unset -> sets core.hooksPath to .githooks
  8. install: refuses when hook file missing (NEW: ensures install never silently arms a no-op hooksPath)
  9. install adapter passes --local to git config (existing)
 10. doctor advice surfaces install command for each drift state (NEW: locks the renderer's reliance on adviceFor() output shape)
 11. repo ships .githooks/pre-commit (existing sanity check)

Mutating command boundary (verified by code reading + fixtures):
- scripts/check-missiond-hooks.mjs: ZERO mutations — always delegates to install-missiond-hooks --check (rejects --install/--dry-fixture flags). Source comment updated to spell out the default-on read-only doctor contract.
- scripts/install-missiond-hooks.mjs: SINGLE mutation point in --install mode — `git config --local core.hooksPath .githooks` via realGit.setCoreHooksPath. --check / default-mode / --dry-fixture never call setCoreHooksPath. Adapter source-string assertion in fixture #9 ensures --local stays in the git config invocation (so it never escapes to --global / --system).
- scripts/render-claudecode-task.mjs: ZERO git-touching code paths — renders preflight commands as text only. Verified by reading the diff: the new renderHooksDoctorPreflight() only pushes literal strings into the lines array; it never imports child_process or invokes git.

Wave 22 protocol followed:
- Claim entry (wave22-01-claim-001) appended to .missiond/tasks/wave22/shared-memory.lisp BEFORE staging.
- Edit-only on in-scope files (no Write); Write used only for this report file.
- Pre-commit gate: scripts/task-scope-guard.mjs --mode staged reported 4 staged files, all inside :write-scope, zero touching :must-not-touch (crates/** .githooks/pre-commit .missiond/v2/*.lisp .missiond/tasks/wave21/**).
- MISSIOND_TASK_CONTRACT env var set on the git commit invocation. The local clone's core.hooksPath is currently NOT .githooks (the doctor reports preflight-drift hooks-path-wrong against /Users/jinchen/Projects/missiond/.git/hooks), so the .githooks/pre-commit hook did not auto-fire — the manual task-scope-guard --mode staged invocation provided the equivalent gate. This is by design: v2 makes the doctor surface drift loudly without forcing the operator's hand.
- Post-commit verify: scripts/verify-task-contract.mjs cross-checked commit 49555c41814b against the contract — message prefix, changed-file scope, must-not-touch intersection, acceptance command presence all green.
- Out-of-scope artifacts (this report file + the wave22 shared-memory.lisp completion-entry edit) intentionally NOT staged; both remain untracked / modified-but-unstaged for the next wave22 archive task.

Constraints honored: NO Rust / SQL / Cargo edits. Did not touch crates/** (a pre-existing modification to crates/missiond-daemon/src/handlers/knowledge/agent_execution.rs from wave22-02's parallel session was observed in the worktree but left untouched and unstaged). Did not touch .missiond/v2/*.lisp. Did not touch .githooks/pre-commit (wave20-01 territory). Did not touch .missiond/tasks/wave21/**. Did not git add . / git stash / git reset / git checkout.")
