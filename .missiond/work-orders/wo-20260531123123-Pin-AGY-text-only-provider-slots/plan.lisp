(work-order-plan
  :schema "missiond.work-order.plan.v1"
  :id "wo-20260531123123-Pin-AGY-text-only-provider-slots"
  :intent "wo-20260531123123-Pin-AGY-text-only-provider-slots"
  :status draft
  :accepted_shards
    ((shard default
       :accepted_shard_id "wo-20260531123123-Pin-AGY-text-only-provider-slots-shard-default"
       :read_scope ["."]
       :write_scope [".missiond/v3/shards/workstation-runtime.lisp"
                     "crates/missiond-daemon/src/provider_box/agy_driver.rs"
                     "crates/missiond-daemon/src/provider_box/http_adapter.rs"]
       :acceptance ["cargo fmt --check"
                    "cargo test -p missiond-daemon provider_box -- --nocapture"
                    "cargo check -p missiond-daemon"
                    "node scripts/check-v3-interactive-provider-box.mjs --json"
                    "node scripts/check-v3-agent-cli-regression.mjs --json"
                    "node scripts/check-v3-workstation-config-isomorphism.mjs --json"])))
