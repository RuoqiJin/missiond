(plan
  :id "20260524-agy-artifact-attribution"
  :schema "missiond.work-order.plan.v1"
  :intent "20260524-agy-artifact-attribution"
  :accepted-shards
    ((shard agy-artifact-task-attribution
       :write-scope ["crates/missiond-daemon/src/engine/intent_engine/autopilot.rs"
                     "crates/missiond-daemon/src/context/v3_contracts/generated.rs"
                     "crates/missiond-daemon/src/context/v3_runtime_defaults/generated.rs"
                     ".missiond/v3/shards/workstation-runtime.lisp"
                     "scripts/check-v3-agent-cli-regression.mjs"
                     "scripts/generated/v3_contracts.d.ts"
                     "scripts/generated/v3_contracts.mjs"
                     "scripts/generated/v3_runtime_defaults.mjs"
                     ".missiond/work-orders/20260524-agy-artifact-attribution/intent.lisp"
                     ".missiond/work-orders/20260524-agy-artifact-attribution/plan.lisp"
                     ".missiond/work-orders/20260524-agy-artifact-attribution/audit.lisp"]
       :must-not-touch ["crates/missiond-core/src/ws/server.rs"
                        "crates/missiond-daemon/src/handlers/knowledge/context_gather.rs"
                        ".mcp.json"]
       :core ((step s1 :logic "detect explicit BoardTask ID in Agy markdown artifacts")
              (step s2 :logic "reject explicit foreign BoardTask artifacts before broad keyword matching")
              (step s3 :logic "prefer exact task-attributed artifacts over broad fallback matches")
              (step s4 :logic "pin behavior with focused regression test and V3 checker anchor"))
       :acceptance ["cargo test -p missiond-daemon agy_artifact"
                    "node scripts/check-v3-agent-cli-regression.mjs --json"
                    "git diff --check"])))
