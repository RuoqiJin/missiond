(work-order-plan
  :schema "missiond.work-order.plan.v1"
  :id "wo-20260526184000-Fix-codex-pty-final-bullet-and-settle"
  :intent "wo-20260526184000-Fix-codex-pty-final-bullet-and-settle"
  :status draft
  :accepted_shards
    ((shard default
       :accepted_shard_id "wo-20260526184000-Fix-codex-pty-final-bullet-and-settle-shard-default"
       :read_scope ["."]
       :write_scope [".missiond/work-orders/wo-20260526184000-Fix-codex-pty-final-bullet-and-settle/**"
                     ".missiond/v3/shards/request-runtime.lisp"
                     ".missiond/v3/generated/**"
                     "crates/missiond-daemon/src/context/v3_contracts/generated.rs"
                     "crates/missiond-daemon/src/context/v3_runtime_defaults/generated.rs"
                     "crates/missiond-daemon/src/engine/intent_engine/autopilot.rs"
                     "scripts/generated/v3_contracts.d.ts"
                     "scripts/generated/v3_contracts.mjs"
                     "scripts/generated/v3_runtime_defaults.mjs"]
       :acceptance ["cargo test -p missiond-daemon idle_watchdog"
                    "cargo test -p missiond-daemon codex_bullet"
                    "node scripts/check-v3-grounded-dispatch-isomorphism.mjs --json"
                    "node scripts/check-v3-final-convergence.mjs --json --static-only"
                    "scripts/cargo-fmt-touched.sh --check"
                    "git diff --check"])))
