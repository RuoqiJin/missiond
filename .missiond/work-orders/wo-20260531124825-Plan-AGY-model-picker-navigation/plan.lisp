(work-order-plan
  :schema "missiond.work-order.plan.v1"
  :id "wo-20260531124825-Plan-AGY-model-picker-navigation"
  :intent "wo-20260531124825-Plan-AGY-model-picker-navigation"
  :status draft
  :accepted_shards
    ((shard default
       :accepted_shard_id "wo-20260531124825-Plan-AGY-model-picker-navigation-shard-default"
       :read_scope ["."]
       :write_scope [".missiond/v3/shards/workstation-runtime.lisp"
                     "crates/missiond-daemon/src/provider_box/agy_driver.rs"]
       :acceptance ["cargo fmt --check"
                    "cargo test -p missiond-daemon provider_box::agy_driver::tests::model_picker -- --nocapture"
                    "cargo test -p missiond-daemon provider_box -- --nocapture"
                    "cargo check -p missiond-daemon"
                    "node scripts/check-v3-interactive-provider-box.mjs --json"
                    "node scripts/check-v3-agent-cli-regression.mjs --json"
                    "node scripts/check-v3-workstation-config-isomorphism.mjs --json"])))
