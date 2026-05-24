(plan
  :id "20260524-agy-numbered-artifact-close"
  :schema "missiond.work-order.plan.v1"
  :intent "20260524-agy-numbered-artifact-close"
  :accepted-shards
    ((shard agy-numbered-artifact-close
       :write-scope [".missiond/v3/shards/request-runtime.lisp"
                     ".missiond/work-orders/20260524-agy-numbered-artifact-close/intent.lisp"
                     ".missiond/work-orders/20260524-agy-numbered-artifact-close/plan.lisp"
                     "crates/missiond-daemon/src/engine/intent_engine/autopilot.rs"
                     "crates/missiond-daemon/src/context/v3_contracts/generated.rs"
                     "crates/missiond-daemon/src/context/v3_runtime_defaults/generated.rs"
                     "scripts/generated/v3_contracts.d.ts"
                     "scripts/generated/v3_contracts.mjs"
                     "scripts/generated/v3_runtime_defaults.mjs"]
       :must-not-touch [".mcp.json" "crates/missiond-core/src/ws/server.rs"]
       :core ((step s1 :logic "pin SSOT invariant that numbered provider report headings are valid output-contract headings")
              (step s2 :logic "normalize leading numeric heading prefixes before matching required report sections")
              (step s3 :logic "add regression tests for numbered output-contract sections and Agy brain artifact completion")
              (step s4 :logic "regenerate typed runtime contracts and run focused plus static gates"))
       :acceptance ["cargo test -p missiond-daemon agy_artifact -- --nocapture"
                    "cargo test -p missiond-daemon output_contract_close_blocker -- --nocapture"
                    "node scripts/check-v3-agent-cli-regression.mjs --json"
                    "node scripts/check-v3-code-isomorphism-complete.mjs --json"
                    "node scripts/check-v3-final-convergence.mjs --json --static-only"
                    "git diff --check"])))
