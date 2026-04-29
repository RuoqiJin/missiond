;; Wave 50 dispatch-time pattern cards.

(pattern-cards wave50-timeout-derived-lease-v0
  :schema "missiond.pattern-cards.dispatch.v0"
  :wave wave50

  (card timeout-projection-single-source
    :use-for [wave50-01-board-task-timeout-lease-v0]
    :summary "Timeout-sensitive runtime values must project from BoardTask.timeout_secs through small pure helpers."
    :recipe ["Do not introduce another literal duration near dispatch."
             "Reuse existing clamp semantics: default 1800s, min 60s, max 7200s."
             "Make BoardTask claim lease cover the pty.send budget plus WATCHDOG_GRACE_SECS, matching smart-watchdog recovery."
             "Add pure helper tests next to existing pty_timeout / idle_watchdog tests."]
    :known-good ["derive_pty_timeout_secs(Some(3300)) == 3300"
                 "idle_watchdog_threshold_secs(Some(3300)) == 3420"])

  (card lisp-code-isomorphism-pin
    :use-for [wave50-01-board-task-timeout-lease-v0]
    :summary "Every runtime invariant change must land with its V3 blueprint text and checker pin."
    :recipe ["Update .missiond/v3/missiond-blueprint.lisp workstation-config note/invariant."
             "Update scripts/check-v3-workstation-config-isomorphism.mjs requireAll + dry fixture."
             "Run the per-surface checker before the aggregate V3 gate."
             "Keep changes scoped to the declared shard write-scope."]
    :known-good ["node scripts/check-v3-workstation-config-isomorphism.mjs"
                 "node scripts/check-v3-code-isomorphism-complete.mjs"])
)
