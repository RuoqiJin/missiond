(work-order-plan
  :schema "missiond.work-order.plan.v1"
  :id "wo-20260531120849-Add-provider-box-slot-control-AP"
  :intent "wo-20260531120849-Add-provider-box-slot-control-AP"
  :status draft
  :accepted_shards
    ((shard default
       :accepted_shard_id "wo-20260531120849-Add-provider-box-slot-control-AP-shard-default"
       :read_scope ["."]
       :write_scope [".missiond/frontend/board-blueprint.lisp"
                     ".missiond/intent-mcp-defs.lisp"
                     ".missiond/v3/shards/request-runtime.lisp"
                     ".missiond/v3/shards/workstation-runtime.lisp"
                     "crates/missiond-daemon/src/provider_box/**"
                     "crates/missiond-daemon/src/handlers/compute/**"
                     "crates/missiond-daemon/src/engine/intent_engine/flow_engine.rs"
                     "crates/missiond-daemon/src/engine/intent_engine/memory_scheduler.rs"
                     "crates/missiond-daemon/src/engine/master_control.rs"
                     "crates/missiond-daemon/src/llm/gemini_driver.rs"
                     "crates/missiond-daemon/src/main.rs"
                     "crates/missiond-daemon/src/slot_orchestrator/cc_controller.rs"
                     "crates/missiond-daemon/src/slot_orchestrator/generic_cli.rs"
                     "crates/missiond-mcp/src/tools/compute/pty.rs"
                     "crates/missiond-pty/src/**"
                     "packages/board/src/app/api/pty/**"
                     "packages/board/src/components/PtyTeachingPanel.tsx"
                     "scripts/check-v3-architecture-boundaries.mjs"
                     "scripts/check-v3-interactive-provider-box.mjs"]
       :acceptance ["node scripts/check-v3-final-convergence.mjs --json --static-only"])))
