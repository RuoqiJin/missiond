(work-order-plan
  :schema "missiond.work-order.plan.v1"
  :id "wo-20260603-missiond-ci-ocaml-toolchain"
  :intent "wo-20260603-missiond-ci-ocaml-toolchain"
  :status draft
  :accepted_shards
    ((shard default
       :accepted_shard_id "wo-20260603-missiond-ci-ocaml-toolchain-shard-default"
       :read_scope ["."]
       :write_scope [".github/workflows/"
                     "scripts/check-missiond-owned-sqlite-clean.mjs"
                     "scripts/check-high-roi-contracts.mjs"
                     ".missiond/work-orders/wo-20260603-missiond-ci-ocaml-toolchain/"]
       :acceptance ["node scripts/check-ocaml-toolchain.mjs --json"
                    "cargo metadata --all-features --format-version 1 --no-deps"
                    "node scripts/check-missiond-owned-sqlite-clean.mjs --json"
                    "node scripts/check-high-roi-contracts.mjs --json"
                    "node scripts/check-pg-migrations-discipline.mjs --json"
                    "node scripts/check-v3-ops-infra-isomorphism.mjs --json"])))
