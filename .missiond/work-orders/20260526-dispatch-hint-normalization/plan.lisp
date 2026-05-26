(plan :id "20260526-dispatch-hint-normalization"
  :steps [
    (step :id "s1" :action "Add dispatch hint normalization helper and focused test")
    (step :id "s2" :action "Update V3 workstation policy and checker anchor")
    (step :id "s3" :action "Run focused checks and commit with work-order")
  ]
  :write_scope [
    "crates/missiond-daemon/src/engine/intent_engine/autopilot.rs"
    "crates/missiond-daemon/src/context/v3_contracts/generated.rs"
    "crates/missiond-daemon/src/context/v3_runtime_defaults/generated.rs"
    ".missiond/v3/shards/workstation-runtime.lisp"
    "scripts/check-v3-workstation-pool-isomorphism.mjs"
    "scripts/generated/v3_contracts.d.ts"
    "scripts/generated/v3_contracts.mjs"
    "scripts/generated/v3_runtime_defaults.mjs"
    ".missiond/work-orders/20260526-dispatch-hint-normalization/**"
  ])
