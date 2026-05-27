(work-order-plan
  :schema "missiond.work-order.plan.v1"
  :id "wo-20260527074302-Control-plane-artifact-first-clo"
  :intent "wo-20260527074302-Control-plane-artifact-first-clo"
  :status draft
  :accepted_shards
    ((shard default
       :accepted_shard_id "wo-20260527074302-Control-plane-artifact-first-clo-shard-default"
       :read_scope ["."]
       :write_scope [".missiond/v3/**"
                     "crates/missiond-core/**"
                     "crates/missiond-daemon/**"
                     "crates/missiond-mcp/**"
                     "packages/board/**"
                     "scripts/**"]
       :acceptance ["node scripts/check-v3-agent-cli-regression.mjs --json"
                    "node scripts/check-v3-grounded-dispatch-isomorphism.mjs --json"
                    "node scripts/check-v3-final-convergence.mjs --json --static-only"
                    "cargo test -p missiond-core jarvis_dispatch_ -- --nocapture"
                    "git diff --check"])))
