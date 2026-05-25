(work-order-plan
  :schema "missiond.work-order.plan.v1"
  :id "interaction-runtime-hardening-20260525"
  :intent "interaction-runtime-hardening-20260525"
  :status draft
  :accepted_shards
    ((shard default
       :accepted_shard_id "interaction-runtime-hardening-20260525-shard-default"
       :read_scope ["."]
       :write_scope ["crates/missiond-core/" "crates/missiond-daemon/" "packages/board/src/types.ts" "scripts/check-v3-interaction-gateway-isomorphism.mjs" "scripts/check-v3-runtime-path-hygiene.mjs" "scripts/check-v3-workstation-pool-isomorphism.mjs" "scripts/compile-v3-runtime.mjs" "scripts/deploy-daemon.sh" "scripts/generated/"]
       :acceptance ["node scripts/check-v3-final-convergence.mjs --json --static-only"
                    "cargo check -p missiond-core"
                    "cargo check -p missiond-daemon"
                    "pnpm --dir packages/board build"
                    "git diff --check"])))
