(work-order-plan
  :schema "missiond.work-order.plan.v1"
  :id "wo-20260602204120-unscoped-evidence-read-model-guard"
  :write_scope [".missiond/v3/shards/request-runtime.lisp"
                "crates/missiond-daemon/src/handlers/knowledge/context_gather.rs"
                "scripts/check-v3-memory-kb-isomorphism.mjs"
                "crates/missiond-daemon/src/context/v3_contracts/generated.rs"
                "crates/missiond-daemon/src/context/v3_runtime_defaults/generated.rs"
                "scripts/generated/v3_contracts.d.ts"
                "scripts/generated/v3_contracts.mjs"
                "scripts/generated/v3_runtime_defaults.mjs"]
  :steps ((step update-v3
           :summary "Add V3 invariant requiring project-scoped persisted evidence_items to be skipped unless project scope or full_debug is present.")
          (step implement-guard
           :summary "Make context_gather skip evidence_items read model for unresolved non-full_debug queries and expose scope_skipped diagnostics.")
          (step verify
           :summary "Run targeted context_gather tests, V3 checkers, deploy, and live smoke unscoped conversation_audit.")))
