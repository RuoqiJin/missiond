(work-order-plan
  :schema "missiond.work-order.plan.v1"
  :id "wo-20260526180312-Fix-idle-watchdog-fresh-pty-final"
  :intent "wo-20260526180312-Fix-idle-watchdog-fresh-pty-final"
  :status draft
  :accepted_shards
    ((shard default
       :accepted_shard_id "wo-20260526180312-Fix-idle-watchdog-fresh-pty-final-shard-default"
       :read_scope ["."]
       :write_scope [".missiond/work-orders/wo-20260526180312-Fix-idle-watchdog-fresh-pty-final/**"
                     ".missiond/v3/shards/request-runtime.lisp"
                     "crates/missiond-daemon/src/engine/intent_engine/autopilot.rs"
                     "crates/missiond-daemon/src/context/v3_contracts/generated.rs"
                     "crates/missiond-daemon/src/context/v3_runtime_defaults/generated.rs"
                     "scripts/generated/v3_contracts.d.ts"
                     "scripts/generated/v3_contracts.mjs"
                     "scripts/generated/v3_runtime_defaults.mjs"]
       :acceptance ["cargo test -p missiond-daemon idle_watchdog"
                    "node scripts/check-v3-final-convergence.mjs --json --static-only"
                    "scripts/cargo-fmt-touched.sh --check"
                    "git diff --check"])))
