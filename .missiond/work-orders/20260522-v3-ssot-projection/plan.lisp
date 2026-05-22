(work-order-plan
  :schema "missiond.work-order.plan.v1"
  :id "20260522-v3-ssot-projection"
  :intent "20260522-v3-ssot-projection"
  :status accepted
  :accepted_shards
    ((shard default
       :accepted_shard_id "20260522-v3-ssot-projection-shard-default"
       :read_scope ["."]
       :write_scope [".missiond/v3/missiond-blueprint.lisp"
                     ".missiond/v3/shards/**"
                     ".missiond/work-orders/20260522-v3-ssot-projection/**"
                     "CLAUDE.md"
                     "README.md"
                     "crates/missiond-daemon/src/context/v3_blueprint_runtime.rs"
                     "scripts/check-typed-lisp-compiler.mjs"
                     "scripts/check-v3-code-isomorphism-complete.mjs"
                     "scripts/check-v3-final-convergence.mjs"
                     "scripts/check-v3-pillar-flow-schema.mjs"
                     "scripts/check-v3-request-lisp-isomorphism.mjs"
                     "scripts/check-v3-v2-coverage.mjs"
                     "scripts/lib/v3_compiled_contract.mjs"]
       :acceptance ["node scripts/check-v3-final-convergence.mjs --json"
                    "cargo test -p missiond-daemon v3_blueprint_runtime"
                    "git diff --check"])))
