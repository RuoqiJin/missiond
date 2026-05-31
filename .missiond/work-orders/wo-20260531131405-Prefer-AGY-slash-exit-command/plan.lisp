(work-order-plan
  :schema "missiond.work-order.plan.v1"
  :id "wo-20260531131405-Prefer-AGY-slash-exit-command"
  :intent "wo-20260531131405-Prefer-AGY-slash-exit-command"
  :status draft
  :accepted_shards
    ((shard default
       :accepted_shard_id "wo-20260531131405-Prefer-AGY-slash-exit-command-shard-default"
       :read_scope ["."]
       :write_scope [".missiond/v3/shards/request-runtime.lisp"
                     "crates/missiond-daemon/src/provider_box/agy_driver.rs"]
       :acceptance ["cargo test -p missiond-daemon provider_box::agy_driver::tests -- --nocapture"
                    "cargo test -p missiond-daemon provider_box -- --nocapture"
                    "cargo fmt --check"
                    "cargo check -p missiond-daemon"
                    "node scripts/check-v3-interactive-provider-box.mjs --json"
                    "node scripts/check-v3-agent-cli-regression.mjs --json"])))
