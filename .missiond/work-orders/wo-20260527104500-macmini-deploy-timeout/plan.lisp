(work-order-plan
  :schema "missiond.work-order.plan.v1"
  :id "wo-20260527104500-macmini-deploy-timeout"
  :intent "wo-20260527104500-macmini-deploy-timeout"
  :status draft
  :accepted_shards
    ((shard default
       :accepted_shard_id "wo-20260527104500-macmini-deploy-timeout-shard-default"
       :read_scope ["."]
       :write_scope ["scripts/deploy-daemon.sh"
                     "scripts/check-v3-ops-infra-isomorphism.mjs"]
       :acceptance ["bash -n scripts/deploy-daemon.sh"
                    "node scripts/check-v3-ops-infra-isomorphism.mjs --json"
                    "node scripts/check-v3-runtime-path-hygiene.mjs --json"
                    "node scripts/check-v3-final-convergence.mjs --json --static-only"
                    "git diff --check"])))
