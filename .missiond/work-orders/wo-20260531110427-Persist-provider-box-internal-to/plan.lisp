(work-order-plan
  :schema "missiond.work-order.plan.v1"
  :id "wo-20260531110427-Persist-provider-box-internal-to"
  :intent "wo-20260531110427-Persist-provider-box-internal-to"
  :status draft
  :accepted_shards
    ((shard default
       :accepted_shard_id "wo-20260531110427-Persist-provider-box-internal-to-shard-default"
       :read_scope ["."]
       :write_scope ["scripts/deploy-daemon.sh"
                     ".missiond/work-orders/wo-20260531110427-Persist-provider-box-internal-to"]
       :acceptance ["bash -n scripts/deploy-daemon.sh"
                    "git diff --check"])))
