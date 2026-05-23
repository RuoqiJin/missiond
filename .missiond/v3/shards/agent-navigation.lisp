(agent-entry-index
  :schema "missiond.agent-entry-index.v1"
  :project_id missiond
  :purpose "Compact task-entry navigation for agents changing MissionD. Entries are authored as small intent anchors; compiled projections join them with semantic facts, source maps, checks, and tool families."
  :rule "This index is navigation only. It does not grant write authority, bypass review gates, or replace Board/workstation/task-result contracts."

  (entry modify-board-backend
    :id modify-board-backend
    :label "Modify Board backend"
    :intent-keywords ["board" "boardtask" "task status" "claim" "note" "decision" "board backend"]
    :primary-family mission_board
    :surfaces [mission_board board-search-noise-governance autopilot-runtime]
    :functions [mission-board board-search-noise-governance delegated-boardtask-runtime]
    :checks ["node scripts/check-v3-board-isomorphism.mjs"
             "node scripts/check-v3-autopilot-runtime-isomorphism.mjs"]
    :write-scope ["crates/missiond-daemon/src/handlers/knowledge/board/**"
                  "crates/missiond-core/src/types/board.rs"
                  "crates/missiond-core/src/db/pg/board.rs"
                  "crates/missiond-mcp/src/tools/knowledge/board.rs"]
    :must-not-touch ["plan execution authority" "workstation slot ownership" "memory provider data"]
    :read-first-override ["crates/missiond-daemon/src/handlers/knowledge/board.rs"
                          "crates/missiond-daemon/src/handlers/knowledge/board/update.rs"
                          "crates/missiond-daemon/src/handlers/knowledge/board/events.rs"
                          "crates/missiond-core/src/types/board.rs"
                          ".missiond/v3/shards/pillar-flow-map.lisp"
                          ".missiond/v3/shards/implementation/runtime-surfaces.lisp"]
    :fallback "If the change affects worker dispatch or completion, switch to modify-workstation-autopilot before editing.")

  (entry modify-board-frontend
    :id modify-board-frontend
    :label "Modify Board frontend"
    :intent-keywords ["board ui" "board frontend" "dashboard" "task dialog" "timeline" "react board"]
    :primary-family mission_board
    :surfaces [board-frontend mission_board]
    :functions [board-frontend mission-board]
    :checks ["node scripts/check-frontend-board-lisp-schema.mjs"
             "node scripts/check-frontend-board-code-isomorphism.mjs"
             "node scripts/check-frontend-board-runtime-projection.mjs"
             "pnpm --dir packages/board build"]
    :write-scope ["packages/board/**"
                  ".missiond/frontend/board-blueprint.lisp"
                  "scripts/project-frontend-board-config.mjs"]
    :must-not-touch ["daemon Board write authority" "runtime slot dispatch" "private memory data"]
    :read-first-override [".missiond/frontend/board-blueprint.lisp"
                          "packages/board/src/App.tsx"
                          "packages/board/src/store.ts"
                          "packages/board/src/api.ts"
                          "scripts/check-frontend-board-code-isomorphism.mjs"]
    :fallback "If the UI needs a backend field that is not projected yet, first use modify-board-backend.")

  (entry modify-plan-execution
    :id modify-plan-execution
    :label "Modify plan execution"
    :intent-keywords ["plan execution" "approve plan" "execute plan" "plan dag" "dry run" "plan runner"]
    :primary-family mission_workflow
    :surfaces [mission_plan review-gate file-artifacts work-order-lifecycle]
    :functions [plan-authoring-and-runner review-gate file-artifact-writer work-order-lifecycle]
    :artifact-contracts [plan task-result-artifact]
    :checks ["node scripts/check-v3-plan-execution-isomorphism.mjs"
             "node scripts/check-v3-review-gate-isomorphism.mjs"
             "node scripts/check-task-contract.mjs --all"]
    :write-scope ["crates/missiond-daemon/src/handlers/knowledge/plan/**"
                  "crates/missiond-daemon/src/handlers/knowledge/plan_dag/**"
                  "crates/missiond-mcp/src/tools/knowledge/plan.rs"]
    :must-not-touch ["mission_request state machine" "workstation provider spawning" "artifact commit outbox migrations unless requested"]
    :read-first-override ["crates/missiond-daemon/src/handlers/knowledge/plan.rs"
                          "crates/missiond-daemon/src/handlers/knowledge/plan/execution_runtime.rs"
                          "crates/missiond-daemon/src/handlers/knowledge/plan_dag/runtime.rs"
                          "crates/missiond-mcp/src/tools/knowledge/plan.rs"
                          ".missiond/v3/shards/request-runtime.lisp"]
    :fallback "If execution dispatch crosses into worker slot selection, also read modify-workstation-autopilot.")

  (entry modify-workstation-autopilot
    :id modify-workstation-autopilot
    :label "Modify workstation/autopilot runtime"
    :intent-keywords ["autopilot" "workstation" "slot" "pty" "boardtask completion" "worker dispatch" "task delegate"]
    :primary-family mission_workstation
    :surfaces [autopilot-runtime workstation-config workstation-pool workstation-dispatch]
    :functions [delegated-boardtask-runtime workstation-config workstation-pool workstation-dispatch]
    :runtime-policies [autopilot-policy compute-runtime-policy flow-runtime-policy]
    :behavior-kinds [worker subprocess event]
    :checks ["node scripts/check-v3-autopilot-runtime-isomorphism.mjs"
             "node scripts/check-v3-workstation-config-isomorphism.mjs"
             "node scripts/check-v3-workstation-pool-isomorphism.mjs"
             "node scripts/check-v3-workstation-dispatch-isomorphism.mjs"]
    :write-scope ["crates/missiond-daemon/src/engine/intent_engine/autopilot.rs"
                  "crates/missiond-daemon/src/handlers/compute/task_delegate.rs"
                  "crates/missiond-daemon/src/handlers/knowledge/workstation_dispatch/**"
                  "crates/missiond-daemon/src/slot_orchestrator/**"]
    :must-not-touch ["Board search semantics" "plan approval authority" "memory review overlay"]
    :read-first-override ["crates/missiond-daemon/src/engine/intent_engine/autopilot.rs"
                          "crates/missiond-daemon/src/handlers/compute/task_delegate.rs"
                          "crates/missiond-daemon/src/handlers/knowledge/workstation_dispatch.rs"
                          ".missiond/v3/shards/workstation-runtime.lisp"]
    :fallback "If the change is only Board CRUD, use modify-board-backend instead.")

  (entry modify-mcp-tool
    :id modify-mcp-tool
    :label "Modify MCP tool surface"
    :intent-keywords ["mcp tool" "tool directory" "tool schema" "compatibility tool" "capability governance"]
    :primary-family mission_tool_directory
    :surfaces [capability-governance mission-shared-memory semantic-ir-compiler]
    :functions [capability-governance mission-shared-memory semantic-ir-compiler]
    :checks ["node scripts/check-v3-capability-governance-isomorphism.mjs"
             "node scripts/check-v3-shared-memory-isomorphism.mjs"
             "node scripts/check-v3-agent-entry-slices.mjs --json"]
    :write-scope ["crates/missiond-mcp/src/tools/**"
                  "crates/missiond-daemon/src/handlers/**"
                  ".missiond/v3/shards/memory-knowledge-runtime.lisp"
                  ".missiond/v3/shards/agent-navigation.lisp"]
    :must-not-touch ["raw Lisp parsing in production Rust" "private provider secrets" "Board mutation without owning family"]
    :read-first-override ["crates/missiond-mcp/src/tools/comm/tool_directory.rs"
                          "crates/missiond-daemon/src/handlers/comm/tool_directory.rs"
                          ".missiond/v3/shards/memory-knowledge-runtime.lisp"
                          "scripts/check-v3-capability-governance-isomorphism.mjs"]
    :fallback "If the tool returns task context, also inspect mission_context_slice under modify-semantic-ir-ssot.")

  (entry modify-memory-provider
    :id modify-memory-provider
    :label "Modify memory provider boundary"
    :intent-keywords ["memory provider" "kb" "knowledge" "review overlay" "context memory" "provider status"]
    :primary-family mission_memory
    :surfaces [memory-provider-boundary memory-kb mission-shared-memory]
    :functions [memory-provider knowledge-memory mission-shared-memory]
    :runtime-policies [memory-kb-policy learning-engine-policy]
    :checks ["node scripts/check-v3-service-extraction-isomorphism.mjs"
             "node scripts/check-v3-memory-kb-isomorphism.mjs"
             "node scripts/check-v3-shared-memory-isomorphism.mjs"]
    :write-scope ["crates/missiond-daemon/src/handlers/knowledge/kb/**"
                  "crates/missiond-daemon/src/handlers/knowledge/memory.rs"
                  "crates/missiond-daemon/src/engine/learning_engine/**"
                  "crates/missiond-core/src/types/knowledge.rs"]
    :must-not-touch ["provider secrets" "tenant data migrations without explicit task" "plan execution authority"]
    :read-first-override [".missiond/v3/shards/memory-knowledge-runtime.lisp"
                          "crates/missiond-daemon/src/handlers/knowledge/memory.rs"
                          "crates/missiond-daemon/src/handlers/knowledge/kb/query.rs"
                          "crates/missiond-core/src/types/knowledge.rs"]
    :fallback "If the change is only context slicing from semantic IR, use modify-semantic-ir-ssot.")

  (entry modify-semantic-ir-ssot
    :id modify-semantic-ir-ssot
    :label "Modify semantic IR and SSOT projections"
    :intent-keywords ["ssot" "semantic ir" "compiled agent slices" "lisp compiler" "source map" "context slice"]
    :primary-family mission_context
    :surfaces [semantic-ir-compiler typed-lisp-compiler mission-shared-memory]
    :functions [semantic-ir-compiler typed-lisp-compiler mission-shared-memory]
    :checks ["node scripts/check-typed-lisp-compiler.mjs"
             "node scripts/compile-v3-runtime.mjs --check --json"
             "node scripts/check-v3-shared-memory-isomorphism.mjs"
             "node scripts/check-v3-agent-entry-slices.mjs --json"]
    :write-scope ["tools/missiond_lispc/**"
                  "scripts/compile-v3-runtime.mjs"
                  "scripts/lib/v3_*.mjs"
                  "crates/missiond-daemon/src/engine/shared_memory.rs"]
    :must-not-touch ["runtime Rust raw Lisp scanners" "generated compiled JSON by hand" "execution authority semantics"]
    :read-first-override ["tools/missiond_lispc/bin/emit_json.ml"
                          "scripts/compile-v3-runtime.mjs"
                          "scripts/lib/v3_compiled_contract.mjs"
                          "crates/missiond-daemon/src/engine/shared_memory.rs"
                          ".missiond/workflows/semantic-ir-shared-memory-convergence.lisp"]
    :fallback "If the change is only MCP routing, use modify-mcp-tool.")

  (entry modify-workflow-delegation
    :id modify-workflow-delegation
    :label "Modify workflow delegation"
    :intent-keywords ["workflow" "delegate" "swarm" "context pack" "accepted shard" "work order"]
    :primary-family mission_workflow
    :surfaces [mission_workflow context-pack work-order-lifecycle external-work-order-gate workstation-dispatch]
    :functions [workflow-distillation context-pack work-order-lifecycle external-work-order-gate workstation-dispatch]
    :artifact-contracts [workflow context-pack work-order]
    :checks ["node scripts/check-v3-workflow-isomorphism.mjs"
             "node scripts/check-v3-context-pack-isomorphism.mjs"
             "node scripts/check-v3-work-order-lifecycle-isomorphism.mjs"
             "node scripts/check-v3-workstation-dispatch-isomorphism.mjs"]
    :write-scope ["crates/missiond-daemon/src/handlers/knowledge/workflow/**"
                  "crates/missiond-daemon/src/handlers/knowledge/workstation_dispatch/**"
                  "scripts/context-pack-*.mjs"
                  ".missiond/workflows/**"]
    :must-not-touch ["plan approval state" "provider memory data" "Board CRUD unless delegation creates tasks"]
    :read-first-override ["crates/missiond-daemon/src/handlers/knowledge/workflow.rs"
                          "crates/missiond-daemon/src/handlers/knowledge/workflow/distill.rs"
                          "crates/missiond-daemon/src/handlers/knowledge/workstation_dispatch/brief.rs"
                          ".missiond/v3/shards/pillar-flow-map.lisp"]
    :fallback "If the change changes plan DAG execution itself, use modify-plan-execution."))
