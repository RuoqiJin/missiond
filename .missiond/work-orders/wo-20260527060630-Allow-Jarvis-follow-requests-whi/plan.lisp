(work-order-plan
  :schema "missiond.work-order.plan.v1"
  :id "wo-20260527060630-Allow-Jarvis-follow-requests-whi"
  :intent "wo-20260527060630-Allow-Jarvis-follow-requests-whi"
  :status draft
  :accepted_shards
    ((shard default
       :accepted_shard_id "wo-20260527060630-Allow-Jarvis-follow-requests-whi-shard-default"
       :read_scope ["."]
       :write_scope ["crates/missiond-core/src/ws/server.rs" ".missiond/v3/shards/request-runtime.lisp" "scripts/check-v3-grounded-dispatch-isomorphism.mjs" ".missiond/v3/runtime/compiled/compiled-v3-blueprint.json" ".missiond/v3/runtime/compiled/compiled-project-universe.json" ".missiond/v3/runtime/compiled/compiled-workflows.json" ".missiond/v3/runtime/project-v3-contracts.json" ".missiond/v3/runtime/project-v3-contracts.lisp"]
       :acceptance ["cargo test -p missiond-core jarvis_follow -- --nocapture" "node scripts/check-v3-grounded-dispatch-isomorphism.mjs --json" "node scripts/check-v3-final-convergence.mjs --json --static-only"])))
