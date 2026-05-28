(work-order-plan
  :schema "missiond.work-order.plan.v1"
  :id "wo-20260528094101-Bind-automated-maintenance-Board"
  :intent "wo-20260528094101-Bind-automated-maintenance-Board"
  :status draft
  :accepted_shards
    ((shard default
       :accepted_shard_id "wo-20260528094101-Bind-automated-maintenance-Board-shard-default"
       :read_scope ["."]
       :write_scope ["crates/missiond-daemon/src/engine/commit_convergence.rs"
                     "crates/missiond-daemon/src/engine/lisp_code_sync.rs"
                     "crates/missiond-daemon/src/engine/nightly_evolution.rs"
                     "crates/missiond-daemon/src/engine/intent_engine/autopilot.rs"
                     "scripts/check-v3-commit-convergence-loop.mjs"
                     "scripts/check-v3-lisp-code-sync-isomorphism.mjs"
                     "scripts/check-v3-nightly-evolution-isomorphism.mjs"]
       :acceptance ["cargo fmt --check -p missiond-daemon"
                    "node scripts/check-v3-commit-convergence-loop.mjs"
                    "node scripts/check-v3-lisp-code-sync-isomorphism.mjs"
                    "node scripts/check-v3-nightly-evolution-isomorphism.mjs"])))
