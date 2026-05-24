(work-order-plan
  :schema "missiond.work-order.plan.v1"
  :id "jarvis-codex-durable-final-artifact"
  :intent "jarvis-codex-durable-final-artifact"
  :status accepted
  :accepted_shards
    ((shard default
       :accepted_shard_id "jarvis-codex-durable-final-artifact-shard-default"
       :read_scope ["."]
       :write_scope [".missiond/v3/shards/request-runtime.lisp" ".missiond/v3/shards/workstation-runtime.lisp" ".missiond/work-orders/jarvis-codex-durable-final-artifact/intent.lisp" ".missiond/work-orders/jarvis-codex-durable-final-artifact/plan.lisp" ".missiond/work-orders/jarvis-codex-durable-final-artifact/audit.lisp" "crates/missiond-daemon/src/engine/intent_engine/autopilot.rs" "crates/missiond-daemon/src/context/v3_contracts/generated.rs" "crates/missiond-daemon/src/context/v3_runtime_defaults/generated.rs" "scripts/generated/v3_contracts.d.ts" "scripts/generated/v3_contracts.mjs" "scripts/generated/v3_runtime_defaults.mjs"]
       :acceptance ["cargo test -p missiond-daemon provider_final_summary" "cargo test -p missiond-core jarvis_task_wait" "node scripts/check-v3-grounded-dispatch-isomorphism.mjs --json" "node scripts/check-v3-behavior-closure.mjs --json" "node scripts/project-v3-contracts.mjs --check --json" "node scripts/check-v3-runtime-domain-projections.mjs --json" "node scripts/check-v3-final-convergence.mjs --json --static-only" "git diff --check"])))
