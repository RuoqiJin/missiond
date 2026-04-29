;; Wave 50 task report.
;; Schema: missiond.report-contract.v1

(report wave50-01-board-task-timeout-lease-v0
  :schema "missiond.report-contract.v1"
  :task_id "wave50-01-board-task-timeout-lease-v0"
  :status done
  :commit_hash "PENDING"
  :files_changed
    ["crates/missiond-daemon/src/engine/intent_engine/autopilot.rs"
     ".missiond/v3/missiond-blueprint.lisp"
     "scripts/check-v3-workstation-config-isomorphism.mjs"
     ".missiond/tasks/wave50/shared-memory.lisp"
     ".missiond/tasks/wave50/session-trace.lisp"
     ".missiond/tasks/wave50/reports/wave50-01-board-task-timeout-lease-v0.report.lisp"]
  :acceptance_results
    [(:command "node scripts/check-v3-workstation-config-isomorphism.mjs --dry-fixture"
              :exit_code 0 :ok true
              :note "Dry fixture passes after adding the new invariant string + fn derive_board_task_lease_secs to the synthetic blueprint and autopilot.rs fixtures.")
     (:command "node scripts/check-v3-workstation-config-isomorphism.mjs"
              :exit_code 0 :ok true
              :note "Live workstation-config Lisp/code isomorphism check passes; the new requireAll lines (blueprint invariant + fn derive_board_task_lease_secs in autopilot.rs) all match.")
     (:command "node scripts/check-v3-code-isomorphism-complete.mjs"
              :exit_code 0 :ok true
              :note "Aggregate V3 gate still green: 7 surfaces graduated, 8 per-surface checkers passed.")
     (:command "cargo check -p missiond-daemon"
              :exit_code 0 :ok true
              :note "Clean build (only pre-existing dead_code/unused-import warnings unrelated to this shard).")
     (:command "cargo test -p missiond-daemon engine::intent_engine::autopilot::tests -- --nocapture"
              :exit_code 0 :ok true
              :note "31 tests pass (24 pre-existing + 7 new lease tests: default-when-absent, default-for-invalid-values, explicit-3300-is-3420, clamps-high, clamps-low, matches-watchdog-threshold, dispatch-no-longer-uses-fixed-20-minute-lease).")
     (:command "node scripts/check-task-report.mjs .missiond/tasks/wave50/reports/wave50-01-board-task-timeout-lease-v0.report.lisp"
              :exit_code 0 :ok true
              :note "Filled in pre-commit; commit_hash placeholder PENDING is replaced post-commit by the verify step.")
     (:command "git diff --check -- crates/missiond-daemon/src/engine/intent_engine/autopilot.rs .missiond/v3/missiond-blueprint.lisp scripts/check-v3-workstation-config-isomorphism.mjs .missiond/tasks/wave50/reports/wave50-01-board-task-timeout-lease-v0.report.lisp"
              :exit_code 0 :ok true
              :note "No whitespace errors in the wave50-01 write scope; perl NUL-byte audit also clean.")]
  :notes "Implements wave50-01-shard-timeout-derived-lease (the single accepted shard in wave50-integration-plan-001 dispatch-group A).\n\nHelper added (crates/missiond-daemon/src/engine/intent_engine/autopilot.rs):\n  fn derive_board_task_lease_secs(timeout_secs: Option<i64>) -> i64 { idle_watchdog_threshold_secs(timeout_secs) }\n\nExact timeout/lease semantics (single source of truth: idle_watchdog_threshold_secs):\n  - timeout_secs == None        -> PTY_TIMEOUT_DEFAULT_SECS + WATCHDOG_GRACE_SECS = 1800 + 120 = 1920s\n  - timeout_secs == Some(0)     -> 1920s (zero/negative fall back to default)\n  - timeout_secs == Some(-300)  -> 1920s\n  - timeout_secs == Some(5)     -> PTY_TIMEOUT_MIN_SECS + WATCHDOG_GRACE_SECS = 60 + 120 = 180s (low clamp)\n  - timeout_secs == Some(60)    -> 180s\n  - timeout_secs == Some(3300)  -> 3300 + 120 = 3420s (the wave31/wave50 55-minute Opus case)\n  - timeout_secs == Some(7200)  -> PTY_TIMEOUT_MAX_SECS + WATCHDOG_GRACE_SECS = 7200 + 120 = 7320s\n  - timeout_secs == Some(86_400) -> 7320s (high clamp)\n\nCall site (autopilot.rs::dispatch_board_tasks): the previous fixed `let lease = (chrono::Utc::now() + chrono::TimeDelta::minutes(20)).to_rfc3339();` is replaced by `let lease_secs = derive_board_task_lease_secs(task.timeout_secs); let lease = (chrono::Utc::now() + chrono::TimeDelta::seconds(lease_secs)).to_rfc3339();` so the BoardTask claim lease, pty.send budget, and smart-watchdog reclaim threshold all project from the same BoardTask.timeout_secs and move together when that field changes. The legacy 20-minute literal is forbidden by a self-contained regression-guard test (`dispatch_no_longer_uses_fixed_20_minute_lease`) that builds the banned needle at runtime so the assertion message itself cannot trip the guard.\n\nV3 invariant updated:\n  - Added a new line to `(workstation-config :invariants ...)` in .missiond/v3/missiond-blueprint.lisp:\n      \"Autopilot BoardTask claim lease MUST equal the smart-watchdog idle-recovery threshold (projected pty.send budget plus grace); the legacy fixed 20-minute lease is forbidden because it lets the watchdog reclaim a slot whose claim is still legitimately ticking inside the declared timeout\"\n  - Updated the `(implementation-map (surface workstation-config :note ...))` paragraph to add `derive_board_task_lease_secs` alongside the existing `derive_pty_timeout_secs / idle_watchdog_threshold_secs` reference and to note that the fixed 20-minute claim lease is gone.\n  - Pinned the new invariant text in scripts/check-v3-workstation-config-isomorphism.mjs (blueprint requireAll: the new invariant prefix; autopilot requireAll: `fn derive_board_task_lease_secs`). The synthetic dry-fixture now also carries the matching invariant text and a `fn derive_board_task_lease_secs() {}` stub so `--dry-fixture` keeps passing.\n\nPattern-card compliance:\n  - timeout-projection-single-source: no new literal duration introduced near dispatch; lease is a pure function of BoardTask.timeout_secs reusing existing clamp semantics; helper tests live next to existing pty_timeout / idle_watchdog tests.\n  - lisp-code-isomorphism-pin: blueprint invariant text + checker requireAll + dry fixture updated together; per-surface checker run before the aggregate V3 gate; changes scoped to the declared shard write-scope (autopilot.rs + blueprint + workstation-config checker).\n\nScope-clean: only the six declared write-scope paths are modified."
  :verification_tier local)
