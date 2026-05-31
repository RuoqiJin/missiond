(work-order-plan
  :schema "missiond.work-order.plan.v1"
  :id "20260531-codex-provider-text-turn"
  :intent "20260531-codex-provider-text-turn"
  :status draft
  :accepted_shards
    ((shard default
       :accepted_shard_id "20260531-codex-provider-text-turn-shard-default"
       :read_scope ["."]
       :write_scope ["crates/missiond-daemon/src/provider_box/codex_driver.rs"
                     ".missiond/v3/shards/workstation-runtime.lisp"
                     "scripts/check-v3-interactive-provider-box.mjs"
                     "scripts/deploy-daemon.sh"
                     ".missiond/work-orders/20260531-codex-provider-text-turn/*"]
       :acceptance ["cargo test -p missiond-daemon provider_box::codex_driver"
                    "bash -n scripts/deploy-daemon.sh"
                    "node scripts/check-v3-interactive-provider-box.mjs --json"
                    "git diff --check"])))
