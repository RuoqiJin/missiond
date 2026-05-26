(work-order-plan
  :schema "missiond.work-order.plan.v1"
  :id "wo-202605260705-runtime-path-context-gather"
  :intent "wo-202605260705-runtime-path-context-gather"
  :status draft
  :accepted_shards
    ((shard default
       :accepted_shard_id "wo-202605260705-runtime-path-context-gather-shard-default"
       :read_scope ["."]
       :write_scope [".missiond/work-orders/wo-202605260705-runtime-path-context-gather/**"
                     ".missiond/v3/evidence/codex-boot-context.lisp"
                     ".missiond/v3/shards/request-runtime.lisp"
                     "crates/missiond-daemon/src/handlers/knowledge/context_gather.rs"
                     "scripts/check-v3-grounded-dispatch-isomorphism.mjs"
                     "scripts/check-v3-runtime-path-hygiene.mjs"]
       :acceptance ["node scripts/check-v3-runtime-path-hygiene.mjs --json"
                    "node scripts/check-v3-grounded-dispatch-isomorphism.mjs --json"
                    "cargo check -p missiond-daemon"
                    "git diff --check"])))
