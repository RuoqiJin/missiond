(work-order-plan
  :schema "missiond.work-order.plan.v1"
  :id "wo-20260528134502-Pin-xjpcode-text-only-paid-CLI-p"
  :intent "wo-20260528134502-Pin-xjpcode-text-only-paid-CLI-p"
  :status draft
  :accepted_shards
    ((shard default
       :accepted_shard_id "wo-20260528134502-Pin-xjpcode-text-only-paid-CLI-p-shard-default"
       :read_scope ["."]
       :write_scope [".missiond/v3/shards/workstation-runtime.lisp"
                     ".missiond/v3/shards/implementation/request-surfaces.lisp"
                     ".missiond/v3/shards/request-runtime.lisp"
                     "crates/missiond-core/src/ws/server.rs"
                     "scripts/check-v3-xjpcode-portable-runtime.mjs"
                     "scripts/deploy-daemon.sh"]
       :acceptance ["node scripts/check-v3-xjpcode-portable-runtime.mjs --json"
                    "node scripts/check-v3-grounded-dispatch-isomorphism.mjs --json"
                    "cargo check -p missiond-core"
                    "node scripts/check-v3-final-convergence.mjs --json --static-only"])))
