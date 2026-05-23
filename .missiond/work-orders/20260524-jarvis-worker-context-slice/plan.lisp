(plan
  :id "20260524-jarvis-worker-context-slice"
  :schema "missiond.work-order.plan.v1"
  :intent "20260524-jarvis-worker-context-slice"
  :accepted-shards
    ((shard jarvis-worker-context-slice
       :write-scope ["crates/missiond-core/src/ws/server.rs"
                     ".missiond/v3/shards/request-runtime.lisp"
                     "crates/missiond-daemon/src/context/v3_contracts/generated.rs"
                     "crates/missiond-daemon/src/context/v3_runtime_defaults/generated.rs"
                     "scripts/generated/v3_contracts.d.ts"
                     "scripts/generated/v3_contracts.mjs"
                     "scripts/generated/v3_runtime_defaults.mjs"
                     ".missiond/work-orders/20260524-jarvis-worker-context-slice/intent.lisp"
                     ".missiond/work-orders/20260524-jarvis-worker-context-slice/plan.lisp"
                     ".missiond/work-orders/20260524-jarvis-worker-context-slice/audit.lisp"]
       :must-not-touch [".mcp.json"
                        "crates/missiond-daemon/src/engine/intent_engine/autopilot.rs"]
       :core ((step s1 :logic "include target engine/pool/write policy/read scope in Jarvis worker prompt")
              (step s2 :logic "include confirmed intent and plan artifact refs in the prompt")
              (step s3 :logic "include a compact accepted execution slice so workers do not re-infer plan semantics")
              (step s4 :logic "pin prompt shape with a focused Rust test and V3 SSOT note"))
       :acceptance ["cargo test -p missiond-core jarvis_worker_prompt_prefers_materialized_context_file"
                    "node scripts/check-v3-final-convergence.mjs --json --static-only"
                    "git diff --check"])))
