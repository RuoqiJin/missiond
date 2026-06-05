(work-order-plan
  :schema "missiond.work-order.plan.v1"
  :id "wo-20260605155328-Codify-Codebase-and-Deploy-Cente"
  :intent "wo-20260605155328-Codify-Codebase-and-Deploy-Cente"
  :status draft
  :accepted_shards
    ((shard default
       :accepted_shard_id "wo-20260605155328-Codify-Codebase-and-Deploy-Cente-shard-default"
       :read_scope ["."]
       :write_scope [".missiond/v3/shards/deployment-closure-plane.lisp"
                     ".missiond/v3/shards/universe/infrastructure.lisp"
                     ".missiond/v3/shards/universe/service-layer-template.lisp"
                     ".missiond/v3/shards/universe/service-runtime.lisp"
                     "scripts/check-deployment-channel-coverage.mjs"]
       :acceptance ["node scripts/check-v3-deployment-closure-plane.mjs --json"
                    "node scripts/compile-v3-runtime.mjs --check --json"
                    "node scripts/check-deployment-channel-coverage.mjs --json"])))
