(work-order-plan
  :schema "missiond.work-order.plan.v1"
  :id "20260522-genome-autopilot-runtime"
  :intent "20260522-genome-autopilot-runtime"
  :status draft
  :accepted_shards
    ((shard default
       :accepted_shard_id "20260522-genome-autopilot-runtime-shard-default"
       :read_scope ["."]
       :write_scope [".gitignore"
                     ".missiond/v3/genome/**"
                     ".missiond/v3/missiond-blueprint.lisp"
                     "Cargo.lock"
                     "Cargo.toml"
                     "crates/missiond-daemon/Cargo.toml"
                     "crates/missiond-daemon/src/bus/v2_subscribers.rs"
                     "crates/missiond-daemon/src/main.rs"
                     "crates/missiond-daemon/src/organism/**"
                     "crates/missiond-genome/**"
                     "crates/missiond-kernel/**"
                     "crates/missiond-organism-runtime/**"
                     "scripts/check-typed-lisp-compiler.mjs"
                     "scripts/check-v3-autopilot-genome-isomorphism.mjs"
                     "scripts/check-v3-code-isomorphism-complete.mjs"
                     "scripts/check-v3-genome-runtime-isomorphism.mjs"
                     "scripts/compile-v3-runtime.mjs"
                     "tools/missiond_lispc/bin/dune"
                     "tools/missiond_lispc/bin/genome_schema.ml"
                     "tools/missiond_lispc/bin/main.ml"]
       :acceptance ["cargo test -p missiond-kernel -p missiond-genome -p missiond-organism-runtime"
                    "node scripts/check-v3-code-isomorphism-complete.mjs"
                    "node scripts/check-v3-final-convergence.mjs"])))
