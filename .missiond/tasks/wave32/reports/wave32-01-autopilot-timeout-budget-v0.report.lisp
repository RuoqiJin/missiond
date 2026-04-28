;; Worker report for wave32-01-autopilot-timeout-budget-v0.
;; Schema: missiond.report-contract.v1

(report wave32-01-autopilot-timeout-budget-v0
  :schema "missiond.report-contract.v1"
  :task_id "wave32-01-autopilot-timeout-budget-v0"
  :status done
  :commit_hash "ef1e57bd6e963951df3875ce5fd38ce8de2e1dfa"
  :files_changed
    ["crates/missiond-daemon/src/engine/intent_engine/autopilot.rs"
     ".missiond/v3/missiond-blueprint.lisp"]

  :acceptance_results
    [(:command "cargo test -p missiond-daemon engine::intent_engine::autopilot::tests"
      :exit_code 0 :ok true
      :detail "11 tests passed (2 pre-existing build_stuck_summary + 9 new timeout/watchdog policy); 1704 filtered.")
     (:command "cargo check -p missiond-daemon"
      :exit_code 0 :ok true
      :detail "compiles clean; no new warnings introduced by autopilot.rs (pre-existing crate dead-code warnings unchanged).")
     (:command "node scripts/check-lisp-blueprint-compression.mjs"
      :exit_code 0 :ok true
      :detail "v1 manifest + v3 blueprint compression check OK after workstation-config invariant + implementation-map note update.")
     (:command "node scripts/check-architecture-lisp.mjs --no-structure .missiond/v3/missiond-blueprint.lisp"
      :exit_code 0 :ok true
      :detail "architecture-lisp shape OK (1 file scanned).")
     (:command "perl -ne 'exit 1 if /\\x00/' crates/missiond-daemon/src/engine/intent_engine/autopilot.rs .missiond/v3/missiond-blueprint.lisp"
      :exit_code 0 :ok true
      :detail "no NUL bytes in either write-scope file.")
     (:command "git diff --check -- crates/missiond-daemon/src/engine/intent_engine/autopilot.rs .missiond/v3/missiond-blueprint.lisp"
      :exit_code 0 :ok true
      :detail "no trailing-whitespace / mixed-indent diff complaints in the staged write scope.")]

  :timeout_derivation_policy
    (:helper "derive_pty_timeout_secs(Option<i64>) -> i64"
     :default_secs 1800
     :min_secs 60
     :max_secs 7200
     :rules
       ["Option::None or Some(v <= 0) -> default 1800s (matches task_delegate::DEFAULT_TIMEOUT_SECS)."
        "Some(v > 0) -> v.clamp(60, 7200) so a misconfigured short timeout still gives the slot a usable budget and a runaway long timeout cannot exceed task_delegate::MAX_TIMEOUT_SECS."
        "derive_pty_timeout_ms(...) returns secs * 1000 with saturating arithmetic for the pty.send call."]
     :wave31_repro
       "55-minute Opus task (timeout_secs=3300) now produces 3_300_000 ms instead of being silently capped at 600_000 ms; pty_timeout_in_range_passes_through pins this regression case.")

  :watchdog_recovery_threshold_policy
    (:helper "idle_watchdog_threshold_secs(Option<i64>) -> i64"
     :rule "idle_threshold = derive_pty_timeout_secs(timeout_secs) + WATCHDOG_GRACE_SECS (120s); the watchdog requires claimed_age >= idle_threshold before reclaiming an idle slot still bound to a running task."
     :grace_secs 120
     :missing_session_probe_secs 120
     :branches
       [(:case "PTY status = Idle"
         :action "reclaim only if claimed_age >= idle_threshold; otherwise let the original pty.send finish."
         :note "this replaces the wave31 hardcoded `claimed_age <= 120` floor that re-dispatched the already-completed Opus task.")
        (:case "PTY status = Busy / Thinking / Responding"
         :action "leave the task alone — slot is actively producing the result."
         :note "previously this branch was unreachable because the outer 120s gate exited the loop before status was even checked.")
        (:case "PTY status = None (no session)"
         :action "reclaim once claimed_age >= 120s probe window — process is gone, so the original send() will never return."
         :note "kept fast per requirement 3 — a missing session must not wait the full configured timeout.")]
     :note_text_change
       "Watchdog board note now says `任务超出配置 timeout/grace（claimed_age=… timeout=… grace=… 工位 … 已 idle）` instead of only `daemon 重启导致 send() 丢失`, matching requirement 4.")

  :blueprint_change
    (:file ".missiond/v3/missiond-blueprint.lisp"
     :workstation_config_invariants_appended
       ["Autopilot pty.send budget MUST project from BoardTask.timeout_secs (default 1800s, clamped 60..7200) — never a fixed 600_000ms — so a delegated long-running task gets the timeout the delegator already declared"
        "Smart watchdog idle-recovery threshold MUST equal the projected pty.send budget plus a small grace (default 120s); only the no-PTY-session branch may reclaim sooner so a missing process can never wedge the slot"]
     :implementation_map_note_changed true
     :implementation_map_note_summary
       "workstation-config surface now lists autopilot.rs alongside compute_slot/task_delegate, and the :note records that pty.send budget + watchdog idle-recovery are projections of BoardTask.timeout_secs (with the 120s no-PTY-session probe window preserved)."
     :status_kept "code-aligned-partial"
     :status_rationale "wave32-01 brings runtime constants into Lisp/projection alignment; remaining workstation-config debt (model-profile cascade, idle-slot reuse) is unchanged.")

  :notes
    "Pure helpers (derive_pty_timeout_secs / derive_pty_timeout_ms / idle_watchdog_threshold_secs) take Option<i64> directly so unit tests need no AppState. Watchdog rewrite preserves the original early no-PTY-session recovery, gates idle reclaim on the new threshold, and explicitly handles the previously-unreachable busy-slot branch. Tests cover: default/zero/negative/low-clamp/high-clamp/in-range timeout derivation, watchdog threshold with default and 55-minute timeouts, regression guard against the legacy 120s floor, and the missing-session probe invariant."

  :trace_references
    [".missiond/tasks/wave32/shared-memory.lisp :: wave32-01-claim-003"
     ".missiond/tasks/wave32/session-trace.lisp :: wave32-01-trace-preamble-read-004"])
