(work-order-plan
  :schema "missiond.work-order.plan.v1"
  :id "wo-20260531173742-Harden-AGY-text-only-provider-bo"
  :intent "wo-20260531173742-Harden-AGY-text-only-provider-bo"
  :status draft
  :accepted_shards
    ((shard default
       :accepted_shard_id "wo-20260531173742-Harden-AGY-text-only-provider-bo-shard-default"
       :read_scope ["."]
       :write_scope [".missiond/v3/shards/request-runtime.lisp"
                     "crates/missiond-daemon/src/context/v3_contracts/generated.rs"
                     "crates/missiond-daemon/src/context/v3_runtime_defaults/generated.rs"
                     "crates/missiond-daemon/src/provider_box"
                     "crates/missiond-pty/src/pty_recognition.rs"
                     "scripts/generated/v3_contracts.d.ts"
                     "scripts/generated/v3_contracts.mjs"
                     "scripts/generated/v3_runtime_defaults.mjs"]
       :acceptance ["cargo test -p missiond-daemon provider_box:: -- --nocapture"
                    "cargo test -p missiond-pty agy_ -- --nocapture"
                    "node scripts/project-v3-contracts.mjs --check"
                    "node scripts/check-v3-interactive-provider-box.mjs"])))
