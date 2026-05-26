(work-order-plan
  :schema "missiond.work-order.plan.v1"
  :id "wo-20260526103417-MissionD-client-channel-runtime-"
  :intent "wo-20260526103417-MissionD-client-channel-runtime-"
  :status draft
  :accepted_shards
    ((shard default
       :accepted_shard_id "wo-20260526103417-MissionD-client-channel-runtime--shard-default"
       :read_scope ["."]
       :write_scope [".missiond/frontend/board-blueprint.lisp"
                     ".missiond/v3/shards/request-runtime.lisp"
                     ".missiond/v3/shards/universe/behavior-closure.lisp"
                     ".missiond/v3/shards/workstation-runtime.lisp"
                     "crates/missiond-core/src/ws/server.rs"
                     "crates/missiond-daemon/src/context/v3_contracts/generated.rs"
                     "crates/missiond-daemon/src/context/v3_runtime_defaults/generated.rs"
                     "packages/board/src/components/ExecDashboard.tsx"
                     "scripts/audit-stale-boardtask-finals.mjs"
                     "scripts/check-v3-grounded-dispatch-isomorphism.mjs"
                     "scripts/check-v3-interaction-gateway-isomorphism.mjs"
                     "scripts/generated/v3_contracts.d.ts"
                     "scripts/generated/v3_contracts.mjs"
                     "scripts/generated/v3_runtime_defaults.mjs"
                     "scripts/smoke-jarvis-intent-plan-dispatch.mjs"]
       :acceptance ["node scripts/check-v3-interaction-gateway-isomorphism.mjs --json"
                    "node scripts/check-v3-grounded-dispatch-isomorphism.mjs --json"
                    "node scripts/check-v3-agent-cli-regression.mjs --json"
                    "node scripts/check-v3-macmini-self-update-lane.mjs --json"
                    "node scripts/check-v3-final-convergence.mjs --json --static-only"])))
