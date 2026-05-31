(work-order-plan
  :schema "missiond.work-order.plan.v1"
  :id "wo-20260531094613-Add-AGY-provider-box-interactive"
  :intent "wo-20260531094613-Add-AGY-provider-box-interactive"
  :status draft
  :accepted_shards
    ((shard default
       :accepted_shard_id "wo-20260531094613-Add-AGY-provider-box-interactive-shard-default"
       :read_scope ["."]
       :write_scope ["crates/missiond-daemon/src/provider_box/agy_driver.rs"
                     "crates/missiond-daemon/src/provider_box/driver.rs"
                     "crates/missiond-daemon/src/provider_box/http_adapter.rs"
                     "crates/missiond-daemon/src/provider_box/runtime.rs"
                     "crates/missiond-daemon/src/provider_box/types.rs"
                     "crates/missiond-pty/src/pty_recognition.rs"]
       :acceptance ["node scripts/check-v3-final-convergence.mjs --json --static-only"])))
