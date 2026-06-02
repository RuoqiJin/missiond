(work-order-plan
  :schema "missiond.work-order.plan.v1"
  :id "wo-deploy-active-ownership-guard"
  :intent "wo-deploy-active-ownership-guard"
  :status draft
  :accepted_shards
    ((shard default
       :accepted_shard_id "wo-deploy-active-ownership-guard-shard-default"
       :read_scope ["."]
       :write_scope ["scripts/deploy-daemon.sh"
                     "scripts/check-missiond-blue-green-deploy.mjs"
                     "scripts/check-v3-ops-infra-isomorphism.mjs"
                     ".missiond/v3/shards/ops-infra.lisp"
                     ".missiond/v3/shards/implementation/ops-surfaces.lisp"]
       :acceptance ["bash -n scripts/deploy-daemon.sh"
                    "node scripts/check-missiond-blue-green-deploy.mjs --json"
                    "node scripts/check-v3-ops-infra-isomorphism.mjs --json"])))
