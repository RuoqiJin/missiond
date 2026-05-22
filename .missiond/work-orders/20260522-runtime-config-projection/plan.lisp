(work-order-plan
  :schema "missiond.work-order.plan.v1"
  :id "20260522-runtime-config-projection"
  :intent "20260522-runtime-config-projection"
  :status accepted
  :accepted_shards
    ((shard default
       :accepted_shard_id "20260522-runtime-config-projection-shard-default"
       :read_scope ["."]
       :write_scope [".missiond/v3/missiond-blueprint.lisp"
                     ".missiond/work-orders/20260522-runtime-config-projection/**"
                     "Cargo.lock"
                     "crates/missiond-daemon/src/context/v3_blueprint_runtime.rs"
                     "crates/missiond-daemon/Cargo.toml"
                     "scripts/check-missiond-blue-green-deploy.mjs"
                     "scripts/check-typed-lisp-compiler.mjs"
                     "scripts/check-v3-code-isomorphism-complete.mjs"
                     "scripts/check-v3-final-convergence.mjs"
                     "scripts/check-v3-router-policy-isomorphism.mjs"
                     "scripts/check-v3-workstation-config-isomorphism.mjs"
                     "scripts/compile-v3-runtime.mjs"
                     "scripts/context-pack-materialize-wave.mjs"
                     "scripts/context-pack-run-wave.mjs"
                     "scripts/deploy-daemon.sh"
                     "scripts/lib/v3_workstation_runtime.mjs"
                     "scripts/task-runner-dispatch.mjs"
                     "scripts/task-runner-submit-dispatch.mjs"
                     "tools/missiond_lispc/bin/emit_json.ml"
                     "tools/missiond_lispc/bin/main.ml"
                     "tools/missiond_lispc/test/parser_golden.ml"]
       :acceptance ["dune runtest --root tools/missiond_lispc"
                    "node scripts/compile-v3-runtime.mjs --json"
                    "node scripts/check-typed-lisp-compiler.mjs --json"
                    "node scripts/check-v3-workstation-config-isomorphism.mjs --json"
                    "node scripts/check-v3-router-policy-isomorphism.mjs --json"
                    "cargo test -p missiond-daemon v3_blueprint_runtime"
                    "node scripts/check-v3-final-convergence.mjs --json --static-only"
                    "git diff --check"])))
