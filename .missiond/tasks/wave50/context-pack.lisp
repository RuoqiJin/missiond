;; Wave 50 context-pack: already-integrated single-shard plan.

(context-pack wave50-context-pack
  :schema "missiond.context-pack.v1"
  :wave wave50
  :purpose "Two-stage context-pack test for a code-worker shard: BoardTask claim lease must project from timeout_secs."
  :write-model append-only
  :sequence 3

  (observation :id wave50-obs-fixed-lease
    :agent codex-parent
    :seq 1
    :at "2026-04-29T08:05:00Z"
    :summary "Live wave49 monitoring showed delegated BoardTasks with timeout_secs=3300 still received a fixed claim lease about 20 minutes ahead. autopilot.rs dispatch_board_tasks currently sets let lease = now + TimeDelta::minutes(20) immediately after claim_board_task succeeds, while pty.send and smart-watchdog already project from derive_pty_timeout_secs / idle_watchdog_threshold_secs. This is a V3 workstation-config drift: timeout budget, watchdog threshold, and BoardTask claim lease must share the same Lisp-owned projection."
    :files ["crates/missiond-daemon/src/engine/intent_engine/autopilot.rs"
            ".missiond/v3/missiond-blueprint.lisp"
            "scripts/check-v3-workstation-config-isomorphism.mjs"])

  (shard-proposal :id wave50-shard-timeout-derived-lease
    :agent codex-parent
    :seq 2
    :at "2026-04-29T08:05:05Z"
    :summary "Implementation shard: replace the fixed 20-minute BoardTask lease in dispatch_board_tasks with a helper derived from BoardTask.timeout_secs. Prefer a pure helper such as derive_board_task_lease_secs(timeout_secs) that delegates to idle_watchdog_threshold_secs(timeout_secs), so the lease covers the pty.send budget plus WATCHDOG_GRACE_SECS. Add unit tests for None default, explicit 3300s -> 3420s, high clamp, and the dispatch code no longer containing TimeDelta::minutes(20). Update V3 blueprint + workstation-config checker so this Lisp invariant cannot drift again."
    :shard timeout-derived-lease
    :owner claudecode
    :write-scope ["crates/missiond-daemon/src/engine/intent_engine/autopilot.rs"
                  ".missiond/v3/missiond-blueprint.lisp"
                  "scripts/check-v3-workstation-config-isomorphism.mjs"]
    :must-not-touch ["packages/**"
                     ".missiond/v1/**"
                     ".missiond/v2/**"
                     ".missiond/tasks/wave48/**"
                     ".missiond/tasks/wave49/**"
                     ".missiond/tasks/wave50/manifest.lisp"
                     ".missiond/tasks/wave50/wave50-*.lisp"
                     ".missiond/tasks/wave50/context-atlas.lisp"
                     ".missiond/tasks/wave50/pattern-cards.lisp"
                     ".missiond/tasks/wave50/context-pack.lisp"
                     ".missiond/claudecode/**"
                     "scripts/check-context-pack.mjs"
                     "scripts/context-pack-append.mjs"
                     "scripts/context-pack-compile-shards.mjs"
                     "scripts/check-v3-context-pack-isomorphism.mjs"]
    :acceptance ["node scripts/check-v3-workstation-config-isomorphism.mjs --dry-fixture"
                 "node scripts/check-v3-workstation-config-isomorphism.mjs"
                 "node scripts/check-v3-code-isomorphism-complete.mjs"
                 "cargo check -p missiond-daemon"
                 "cargo test -p missiond-daemon engine::intent_engine::autopilot::tests -- --nocapture"
                 "git diff --check -- crates/missiond-daemon/src/engine/intent_engine/autopilot.rs .missiond/v3/missiond-blueprint.lisp scripts/check-v3-workstation-config-isomorphism.mjs"])

  (integration-plan :id wave50-integration-plan-001
    :agent codex-integrator
    :seq 3
    :at "2026-04-29T08:05:10Z"
    :summary "Accepted one implementation shard. This wave deliberately uses mapped dispatch-groups so the worker brief can consume context-pack-compile-shards output without narrative parsing."
    :accepted-shards [timeout-derived-lease]
    :dispatch-groups [(group :id A :shards [timeout-derived-lease])])
)
