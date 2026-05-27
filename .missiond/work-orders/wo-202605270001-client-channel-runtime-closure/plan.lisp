(work-order-plan
  :schema "missiond.work-order.plan.v1"
  :id "wo-202605270001-client-channel-runtime-closure"
  :intent "wo-202605270001-client-channel-runtime-closure"
  :status draft
  :accepted_shards
    ((shard jarvis-follow
       :accepted_shard_id "wo-202605270001-client-channel-runtime-closure-shard-jarvis-follow"
       :read_scope ["."]
       :write_scope ["scripts/smoke-jarvis-intent-plan-dispatch.mjs"
                     "packages/board/src/components/JarvisChat.tsx"
                     "packages/board/src/components/ExecDashboard.tsx"
                     "packages/board/src/eventStream.ts"
                     "packages/board/src/hooks/useEventStream.ts"
                     "packages/board/src/components/OperationsOverview.tsx"
                     "crates/missiond-core/src/ws/server.rs"
                     ".missiond/v3/shards/request-runtime.lisp"
                     ".missiond/v3/shards/universe/behavior-closure.lisp"
                     ".missiond/frontend/board-blueprint.lisp"
                     "scripts/check-v3-interaction-gateway-isomorphism.mjs"
                     "scripts/check-v3-grounded-dispatch-isomorphism.mjs"
                     "scripts/check-frontend-board-runtime-projection.mjs"
                     "crates/missiond-daemon/src/context/v3_contracts/generated.rs"
                     "crates/missiond-daemon/src/context/v3_runtime_defaults/generated.rs"
                     "scripts/generated/v3_contracts.d.ts"
                     "scripts/generated/v3_contracts.mjs"
                     "scripts/generated/v3_runtime_defaults.mjs"
                     ".missiond/work-orders/wo-202605270001-client-channel-runtime-closure/intent.lisp"
                     ".missiond/work-orders/wo-202605270001-client-channel-runtime-closure/plan.lisp"
                     ".missiond/work-orders/wo-202605270001-client-channel-runtime-closure/audit.lisp"]
       :acceptance ["node scripts/check-v3-interaction-gateway-isomorphism.mjs --json"
                    "node scripts/check-v3-grounded-dispatch-isomorphism.mjs --json"
                    "node scripts/check-v3-final-convergence.mjs --json --static-only"
                    "pnpm --dir packages/board build"]))
