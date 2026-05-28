(work-order-plan
  :schema "missiond.work-order.plan.v1"
  :id "wo-20260528065000-Plan-DAG-work-lease-kernel"
  :intent "wo-20260528065000-Plan-DAG-work-lease-kernel"
  :status draft
  :accepted_shards
    ((shard default
       :accepted_shard_id "wo-20260528065000-Plan-DAG-work-lease-kernel-shard-default"
       :read_scope ["."]
       :write_scope ["crates/missiond-daemon/src/handlers/knowledge/plan_dag/claim_lease.rs"
                     "crates/missiond-daemon/src/handlers/knowledge/agent_execution.rs"
                     "crates/missiond-daemon/src/handlers/knowledge/plan_dag.rs"
                     "crates/missiond-daemon/src/handlers/knowledge/plan_dag/runtime.rs"
                     "crates/missiond-daemon/src/handlers/knowledge/plan_dag/runtime/claiming.rs"
                     "crates/missiond-daemon/src/handlers/knowledge/plan_dag/runtime/claims.rs"
                     "crates/missiond-daemon/src/handlers/knowledge/plan_dag/runtime/drain.rs"
                     "crates/missiond-daemon/src/handlers/knowledge/plan_dag/runtime/retry.rs"
                     "crates/missiond-daemon/src/handlers/knowledge/plan_dag/lifecycle/claims.rs"
                     "crates/missiond-daemon/src/handlers/knowledge/plan_dag/outcome/state.rs"
                     "crates/missiond-daemon/src/handlers/knowledge/plan_dag/tests.rs"
                     "scripts/check-v3-control-plane-kernel-isomorphism.mjs"]
       :acceptance ["node scripts/check-v3-control-plane-kernel-isomorphism.mjs --json"
                    "cargo check -p missiond-daemon"
                    "git diff --check"])))
