(agent-entry-index
  :schema "missiond.agent-entry-index.v1"
  :project_id missiond
  :purpose "Compact task-entry navigation for agents changing MissionD. Entries are authored as small intent anchors; compiled projections join them with semantic facts, source maps, checks, and tool families."
  :rule "This index is navigation only. It does not grant write authority, bypass review gates, or replace Board/workstation/task-result contracts."

  (agent-navigation-policy closure
    :schema "missiond.agent-navigation-policy.v1"
    :review-sidecar ".missiond/v3/runtime/agent-navigation-review.json"
    :project-navigation-artifact ".missiond/v3/runtime/compiled/compiled-project-agent-navigation.json"
    :quality-corpus ".missiond/v3/evidence/agent-navigation-quality-corpus.json"
    :min-fixtures-per-entry 2
    :min-match-accuracy 1.0
    :project-mode read-only-registered-projects
    :actions [catalog review feedback suggest_entries]
    :rule "mission_agent_navigation may write only the review sidecar; it must not mutate SSOT shards, Board, KB, project registries, or sibling repositories. Project navigation for non-MissionD projects is derived read-only from the project registry and project-local SSOT paths."
    :checks ["node scripts/check-v3-agent-navigation-quality.mjs --json --check"
             "node scripts/check-v3-agent-navigation-closure.mjs --json"])

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
    :intent-keywords ["autopilot" "workstation" "slot" "pty" "boardtask completion" "boardtask 完成" "完成判定" "worker dispatch" "task delegate"]
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
    :fallback "If the change changes plan DAG execution itself, use modify-plan-execution.")

  (entry modify-request-runtime
    :id modify-request-runtime
    :label "Modify request runtime"
    :intent-keywords ["request runtime" "mission_request" "jarvis chat" "sse" "review packet" "request state"]
    :primary-family mission_workflow
    :surfaces [mission_request conversation-ingestion resident-master-control]
    :functions [request-lifecycle conversation-ingestion resident-master-control]
    :artifact-contracts [mission-request task-result-artifact]
    :runtime-policies [conversation-ingestion-policy]
    :checks ["node scripts/check-v3-request-lisp-isomorphism.mjs"
             "node scripts/check-v3-request-flow-smoke.mjs --json"
             "node scripts/check-v3-conversation-ingestion-isomorphism.mjs"]
    :write-scope ["crates/missiond-daemon/src/handlers/knowledge/request/**"
                  "crates/missiond-daemon/src/handlers/comm/conversation/**"
                  "crates/missiond-core/src/ws/server.rs"
                  ".missiond/v3/shards/request-runtime.lisp"]
    :must-not-touch ["plan approval authority" "workstation slot allocation" "private provider secrets"]
    :read-first-override [".missiond/v3/shards/request-runtime.lisp"
                          "crates/missiond-daemon/src/handlers/knowledge/request.rs"
                          "crates/missiond-daemon/src/handlers/knowledge/request/respond.rs"
                          "crates/missiond-core/src/ws/server.rs"]
    :fallback "If the change is only task-result artifact persistence, use modify-file-artifacts.")

  (entry modify-file-artifacts
    :id modify-file-artifacts
    :label "Modify file and task-result artifacts"
    :intent-keywords ["file artifact" "task-result-artifact" "artifact commit" "result artifact" "shared artifact" "outbox"]
    :primary-family mission_context
    :surfaces [file-artifacts evidence-governance-view mission-shared-memory]
    :functions [file-artifact-writer evidence-governance-view mission-shared-memory]
    :artifact-contracts [task-result-artifact final-report verification-receipt]
    :checks ["node scripts/check-v3-file-artifacts-isomorphism.mjs"
             "node scripts/check-v3-evidence-collector-isomorphism.mjs"
             "node scripts/check-v3-shared-memory-isomorphism.mjs"]
    :write-scope ["crates/missiond-daemon/src/handlers/knowledge/file_artifacts/**"
                  "crates/missiond-daemon/src/engine/shared_memory.rs"
                  "crates/missiond-core/src/db/artifact_commit.rs"
                  "crates/missiond-core/src/db/pg/artifact_commit.rs"]
    :must-not-touch ["Board task status semantics" "provider conversation storage" "artifact compiled JSON by hand"]
    :read-first-override ["crates/missiond-daemon/src/handlers/knowledge/file_artifacts.rs"
                          "crates/missiond-daemon/src/handlers/knowledge/file_artifacts/commit.rs"
                          "crates/missiond-daemon/src/engine/shared_memory.rs"
                          ".missiond/v3/shards/control-plane-runtime.lisp"]
    :fallback "If the artifact is consumed by request SSE completion, also read modify-request-runtime.")

  (entry modify-review-gate
    :id modify-review-gate
    :label "Modify review gate"
    :intent-keywords ["review gate" "approve" "approval" "plan review" "decision gate" "automation review"]
    :primary-family mission_workflow
    :surfaces [review-gate mission_plan decision-inbox-revalidation]
    :functions [review-gate plan-authoring-and-runner decision-inbox-revalidation]
    :artifact-contracts [plan workflow]
    :checks ["node scripts/check-v3-review-gate-isomorphism.mjs"
             "node scripts/check-v3-plan-execution-isomorphism.mjs"
             "node scripts/check-v3-task-lifecycle-isomorphism.mjs"]
    :write-scope ["crates/missiond-daemon/src/handlers/knowledge/review_gate/**"
                  "crates/missiond-daemon/src/handlers/knowledge/plan/**"
                  "crates/missiond-mcp/src/tools/knowledge/plan.rs"]
    :must-not-touch ["worker dispatch slots" "Board CRUD authority" "memory provider data"]
    :read-first-override ["crates/missiond-daemon/src/handlers/knowledge/review_gate.rs"
                          "crates/missiond-daemon/src/handlers/knowledge/review_gate/resolution/automation.rs"
                          "crates/missiond-daemon/src/handlers/knowledge/plan/approval_review.rs"
                          ".missiond/v3/shards/request-runtime.lisp"]
    :fallback "If approval changes execute plan DAG nodes, also use modify-plan-execution.")

  (entry modify-project-registry
    :id modify-project-registry
    :label "Modify project registry"
    :intent-keywords ["project registry" "project_id" "universe" "registered project" "project root" "import universe"]
    :primary-family mission_universe
    :surfaces [project-registry data-residency-universe eventhub-service-boundary]
    :functions [project-registry data-residency-universe eventhub-service]
    :runtime-policies [project-registry-policy]
    :checks ["node scripts/check-v3-project-registry-isomorphism.mjs"
             "node scripts/check-project-ssot-universe.mjs --json"
             "node scripts/check-v3-data-residency-universe-isomorphism.mjs --json"]
    :write-scope ["crates/missiond-daemon/src/handlers/knowledge/project/**"
                  ".missiond/v3/shards/universe/project-registry.lisp"
                  "scripts/check-project-*.mjs"]
    :must-not-touch ["sibling repository files" "deployment secrets" "deploy-center authority facts"]
    :read-first-override [".missiond/v3/shards/universe/project-registry.lisp"
                          "crates/missiond-daemon/src/handlers/knowledge/project.rs"
                          "crates/missiond-daemon/src/handlers/knowledge/project/registry.rs"
                          "scripts/check-v3-project-registry-isomorphism.mjs"]
    :fallback "If the change is project navigation only, use modify-semantic-ir-ssot and keep sibling repos read-only.")

  (entry modify-router-policy
    :id modify-router-policy
    :label "Modify router policy"
    :intent-keywords ["router" "model router" "embedding" "rerank" "provider" "quota" "usage ledger"]
    :primary-family mission_router
    :surfaces [router-policy compute-primitives conversation-ingestion]
    :functions [router-policy compute-primitives conversation-ingestion]
    :runtime-policies [router-runtime-policy minimax-runtime-policy compute-runtime-policy]
    :checks ["node scripts/check-v3-router-policy-isomorphism.mjs"
             "node scripts/check-v3-compute-primitives-isomorphism.mjs"
             "node scripts/check-router-policy.mjs"]
    :write-scope ["crates/missiond-daemon/src/handlers/comm/router_chat/**"
                  "crates/missiond-daemon/src/llm/**"
                  "crates/missiond-mcp/src/tools/comm/router_chat.rs"
                  ".missiond/v3/shards/control-plane-runtime.lisp"]
    :must-not-touch ["provider credentials" "billing configuration" "deployment DNS"]
    :read-first-override [".missiond/v3/shards/control-plane-runtime.lisp"
                          "crates/missiond-daemon/src/handlers/comm/router_chat.rs"
                          "crates/missiond-daemon/src/handlers/comm/router_chat/chat.rs"
                          "scripts/check-v3-router-policy-isomorphism.mjs"]
    :fallback "If the issue is only PTY provider-unavailable recognition, use modify-workstation-autopilot.")

  (entry modify-ops-infra
    :id modify-ops-infra
    :label "Modify ops and infrastructure surfaces"
    :intent-keywords ["ops" "infra" "deploy" "daemon update" "system logs" "power control" "blue green"]
    :primary-family mission_ops
    :surfaces [ops-infra sysinfra-control missiond-blue-green-self-update]
    :functions [ops-infra sysinfra-control missiond-blue-green-self-update]
    :checks ["node scripts/check-v3-ops-infra-isomorphism.mjs"
             "node scripts/check-v3-sysinfra-control-isomorphism.mjs"
             "node scripts/check-missiond-blue-green-deploy.mjs --json"]
    :write-scope ["crates/missiond-daemon/src/handlers/sysinfra/**"
                  "crates/missiond-mcp/src/tools/sysinfra/**"
                  "scripts/deploy-daemon.sh"
                  ".missiond/v3/shards/ops-infra.lisp"]
    :must-not-touch ["production secrets" "Cloudflare DNS" "provider billing"]
    :read-first-override [".missiond/v3/shards/ops-infra.lisp"
                          "crates/missiond-daemon/src/handlers/sysinfra/system.rs"
                          "crates/missiond-daemon/src/handlers/sysinfra/infra.rs"
                          "scripts/check-v3-ops-infra-isomorphism.mjs"]
    :fallback "If the change is project identity rather than runtime ops, use modify-project-registry.")

  (entry modify-conversation-ingestion
    :id modify-conversation-ingestion
    :label "Modify conversation ingestion"
    :intent-keywords ["conversation ingestion" "codex ops" "conversation list" "message search" "timeline trace" "conversation labels"]
    :primary-family mission_context
    :surfaces [conversation-ingestion codex-boot-context evidence-governance-view]
    :functions [conversation-ingestion codex-boot-context evidence-governance-view]
    :runtime-policies [conversation-ingestion-policy]
    :checks ["node scripts/check-v3-conversation-ingestion-isomorphism.mjs"
             "node scripts/check-v3-cli-conversation-ingestion-isomorphism.mjs"
             "node scripts/check-v3-evidence-collector-isomorphism.mjs"]
    :write-scope ["crates/missiond-daemon/src/handlers/comm/conversation/**"
                  "crates/missiond-daemon/src/workers/local/codex_ingestion_worker.rs"
                  "crates/missiond-core/src/db/pg/conversation.rs"
                  "packages/board/src/components/Conversations.tsx"]
    :must-not-touch ["memory review overlay" "Board execution authority" "provider credentials"]
    :read-first-override [".missiond/v3/shards/memory-knowledge-runtime.lisp"
                          "crates/missiond-daemon/src/handlers/comm/conversation.rs"
                          "crates/missiond-daemon/src/workers/local/codex_ingestion_worker.rs"
                          "packages/board/src/components/Conversations.tsx"]
    :fallback "If conversation facts are used only as context slices, use modify-semantic-ir-ssot.")

  (entry modify-source-hygiene-and-work-order
    :id modify-source-hygiene-and-work-order
    :label "Modify source hygiene and work-order gate"
    :intent-keywords ["work order" "commit hook" "source hygiene" "task scope" "staged source" "MissionD-Work-Order"]
    :primary-family mission_ops
    :surfaces [source-hygiene work-order-lifecycle external-work-order-gate]
    :functions [source-hygiene work-order-lifecycle external-work-order-gate]
    :artifact-contracts [work-order]
    :checks ["node scripts/check-v3-source-hygiene-isomorphism.mjs"
             "node scripts/check-v3-work-order-lifecycle-isomorphism.mjs"
             "node scripts/check-staged-source-hygiene.mjs --files scripts/check-staged-source-hygiene.mjs"]
    :write-scope ["scripts/check-staged-source-hygiene.mjs"
                  "scripts/task-scope-guard.mjs"
                  "scripts/missiond-work-order.mjs"
                  ".githooks/**"
                  ".missiond/work-orders/**"]
    :must-not-touch ["unrelated staged files" "user dirty worktree changes" "generated compiled JSON by hand"]
    :read-first-override [".missiond/v3/shards/implementation/ops-surfaces.lisp"
                          ".missiond/workflows/work-order-lifecycle.lisp"
                          "scripts/missiond-work-order.mjs"
                          "scripts/check-staged-source-hygiene.mjs"]
    :fallback "If the work-order gate blocks a concrete implementation, create or update that task's work order rather than weakening the hook."))
