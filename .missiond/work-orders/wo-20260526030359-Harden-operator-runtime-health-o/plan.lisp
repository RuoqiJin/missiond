(work-order-plan
  :schema "missiond.work-order.plan.v1"
  :id "wo-20260526030359-Harden-operator-runtime-health-o"
  :intent "wo-20260526030359-Harden-operator-runtime-health-o"
  :status draft
  :accepted_shards
    ((shard default
       :accepted_shard_id "wo-20260526030359-Harden-operator-runtime-health-o-shard-default"
       :read_scope ["."]
       :write_scope [".github/workflows/quality-gates.yml"
                     "crates/missiond-core/migrations/20260526000000_worker_runtime_state.sql"
                     "crates/missiond-daemon/src/bus/bootstrap.rs"
                     "crates/missiond-daemon/src/bus/ws_bridge.rs"
                     "crates/missiond-daemon/src/engine/shared_memory.rs"
                     "crates/missiond-daemon/src/handlers/compute/worker.rs"
                     "crates/missiond-daemon/src/handlers/sysinfra/misc.rs"
                     "crates/missiond-daemon/src/main.rs"
                     "crates/missiond-daemon/src/state.rs"
                     "crates/missiond-daemon/src/workers/registry.rs"
                     "packages/board/src/app/api/operator/overview/route.ts"
                     "packages/board/src/components/OperationsOverview.tsx"
                     "packages/board/src/lib/operatorOverview.ts"
                     "scripts/check-high-roi-contracts.mjs"
                     "scripts/check-missiond-owned-sqlite-clean.mjs"
                     "scripts/check-pg-migrations-discipline.mjs"]
       :acceptance ["cargo check -p missiond-daemon -p missiond-core -p skill-store"
                   "cargo test -p missiond-daemon workers::registry::tests"
                   "pnpm --dir packages/board typecheck"
                   "pnpm --dir packages/board build"
                   "node scripts/check-v3-ops-infra-isomorphism.mjs --json"
                   "node scripts/check-missiond-owned-sqlite-clean.mjs --json"
                   "node scripts/check-v3-cli-conversation-ingestion-isomorphism.mjs --json"
                   "node scripts/check-high-roi-contracts.mjs --json"
                   "node scripts/check-pg-migrations-discipline.mjs --json"])))
