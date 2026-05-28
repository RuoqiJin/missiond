(work-order-plan
  :schema "missiond.work-order.plan.v1"
  :id "wo-20260528094835-Bind-diagnostic-BoardTasks-to-ta"
  :intent "wo-20260528094835-Bind-diagnostic-BoardTasks-to-ta"
  :status draft
  :accepted_shards
    ((shard default
       :accepted_shard_id "wo-20260528094835-Bind-diagnostic-BoardTasks-to-ta-shard-default"
       :read_scope ["."]
       :write_scope ["crates/missiond-daemon/src/infra/aiops.rs"
                     "crates/missiond-daemon/src/handlers/comm/question/llm_trace.rs"
                     "crates/missiond-daemon/src/handlers/knowledge/request/respond/materialization.rs"
                     "scripts/check-v3-board-isomorphism.mjs"
                     "scripts/check-v3-incident-governance-isomorphism.mjs"
                     "scripts/check-v3-request-lisp-isomorphism.mjs"]
       :acceptance ["cargo fmt --check -p missiond-daemon"
                    "node scripts/check-v3-board-isomorphism.mjs --json"
                    "node scripts/check-v3-incident-governance-isomorphism.mjs --json"
                    "node scripts/check-v3-request-lisp-isomorphism.mjs --json"])))
