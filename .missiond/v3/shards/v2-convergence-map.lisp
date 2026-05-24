  (v2-convergence-map
    :schema "missiond.v2-convergence-map.v1"
    :status-enum [missing designed code-aligned runtime-projected]
    :rule "V2 is historical evidence, not an implementation authority. Every effective V2 design item must name its V3 pillar/function/surface destination; :status missing is forbidden. designed means the V3 destination exists but the code is not yet physically/runtime aligned; code-aligned and runtime-projected must point at code-aligned implementation surfaces."

    (v2-item intent-alignment-plan-execution-loop
      :status code-aligned
      :v2-source ".missiond/v2/intent-flow.lisp :: F-intent-alignment-plan-execution-loop"
      :v3-pillar request
      :v3-function request-lifecycle
      :surface mission_request
      :note "The V2 user intent -> alignment -> plan -> execute loop is now the request-local mission_request review-packet/respond entry.")
    (v2-item unified-entry-runtime-bridge
      :status code-aligned
      :v2-source ".missiond/v2/intent.lisp :: unified-entry-pipeline-v1"
      :v3-pillar request
      :v3-function unified-entry-runtime
      :surface unified-entry-runtime
      :note "V2 unified-entry run-pipeline helper is the daemon-local bridge behind mission_request.")
    (v2-item external-channel-interaction-gateway
      :status code-aligned
      :v2-source ".missiond/v2/intent.lisp :: Jarvis/web/mobile/external user entry"
      :v3-pillar communication
      :v3-function interaction-gateway
      :surface interaction-gateway
      :note "External human/service channels now normalize to InteractionEnvelope before Auth identity, permission context, grounding, intent/plan confirmation, BoardTask dispatch, task-result-artifact collection, and channel response sinks.")
    (v2-item file-first-artifacts
      :status code-aligned
      :v2-source ".missiond/v2/architecture-dsl.lisp :: l2-shard-split-plan/intent-directive-artifacts"
      :v3-pillar artifacts
      :v3-function file-artifact-writer
      :surface file-artifacts
      :note "V2 file-first writer discipline is centralized as the V3 file-artifacts surface.")
    (v2-item directive-alignment-authoring
      :status code-aligned
      :v2-source ".missiond/v2/intent.lisp :: directive-layer/action-instruction-actor"
      :v3-pillar intent
      :v3-function directive-authoring
      :surface mission_directive
      :note "V2 directive/alignment actor maps to mission_directive authoring plus review-gate emission.")
    (v2-item plan-authoring-and-dag-runner
      :status code-aligned
      :v2-source ".missiond/v2/intent.lisp :: plan-dag-runtime-v2"
      :v3-pillar plan
      :v3-function plan-authoring-and-runner
      :surface mission_plan
      :note "V2 PLAN DAG runtime and plan compiler are the V3 mission_plan author/run surface.")
    (v2-item evidence-collector-event-ref
      :status code-aligned
      :v2-source ".missiond/v2/intent.lisp :: evidence-collector-typed"
      :v3-pillar verification
      :v3-function evidence-collector
      :surface evidence-collector
      :note "V2 EvidenceEntry/event-ref design is pinned in the typed V3 evidence collector.")
    (v2-item execution-log-governance
      :status code-aligned
      :v2-source ".missiond/v2/architecture-dsl.lisp :: intent-execution-governance / F-execution-log-governance"
      :v3-pillar execution
      :v3-function execution-log
      :surface mission_execution-log
      :note "V2 companion log and execution event bus become the V3 mission_execution-log function.")
    (v2-item execution-claim-lease
      :status code-aligned
      :v2-source ".missiond/v2/intent-execution-governance.lisp :: claim lease"
      :v3-pillar execution
      :v3-function execution-claim-lease
      :surface mission_execution-claim-lease
      :note "V2 claim/heartbeat/release mechanics are explicit under the execution pillar.")
    (v2-item scoped-commit-completion-audit
      :status code-aligned
      :v2-source ".missiond/v2/architecture-dsl.lisp :: F-scoped-commit-handoff"
      :v3-pillar execution
      :v3-function execution-completion-audit
      :surface mission_execution-completion-audit
      :note "V2 scoped commit handoff and daemon verifier inputs converge into V3 completion audit.")
    (v2-item workflow-distillation
      :status code-aligned
      :v2-source ".missiond/v2/intent.lisp :: workflow specs / F-methodology-to-executable-compile"
      :v3-pillar workflow
      :v3-function workflow-distillation
      :surface mission_workflow
      :note "V2 workflow/methodology distillation remains V3 mission_workflow.")
    (v2-item typed-lisp-compiler-gate
      :status code-aligned
      :v2-source ".missiond/v2/intent.lisp :: checker/token-pin debt and Lisp SSOT schema evidence"
      :v3-pillar workflow
      :v3-function typed-lisp-compiler
      :surface typed-lisp-compiler
      :note "The typed Lisp compiler is the V3 destination for fragile V2-era Lisp shape/token-pin checking; Lisp remains SSOT while OCaml owns typed AST diagnostics and generated projections.")
    (v2-item genome-driven-organ-runtime
      :status code-aligned
      :v2-source ".missiond/v2/intent-event-bus.lisp :: event-bus subscriber handoff and delegated-boardtask autopilot"
      :v3-pillar workflow
      :v3-function genome-runtime
      :surface genome-runtime
      :note "The genome runtime is the V3 destination for moving hand-written subscriber predicates into Lisp-declared Genome receptors/effects and Rust Atom/Cell/Tissue/Organ execution, starting with Autopilot shadow parity before active cutover.")
    (v2-item semantic-ir-compiler-projection
      :status code-aligned
      :v2-source ".missiond/v2/intent.lisp :: checker/token-pin debt and worker context projection"
      :v3-pillar workflow
      :v3-function semantic-ir-compiler
      :surface semantic-ir-compiler
      :note "The V3 semantic IR compiler projects typed surface/function facts, source maps, compact agent slices, and workflow contracts so JS checkers and workers consume structured facts instead of prose/token pins.")
    (v2-item work-order-lifecycle
      :status code-aligned
      :v2-source ".missiond/v2/intent-flow.lisp :: F-intent-alignment-plan-execution-loop + .missiond/v2/intent.lisp :: unified-entry-pipeline-v1"
      :v3-pillar workflow
      :v3-function work-order-lifecycle
      :surface work-order-lifecycle
      :note "V2's split intent/plan/execute loops and unified-entry pipeline converge into one work-order lifecycle. User requests, BoardTask triggers, intent.lisp files, and external-application delegation are normalized into a work-order intent, bound to one BoardTask, compiled into plan.lisp accepted shards, executed through workflow_run/shared-memory, and closed via task-result-artifacts plus audit.lisp. The lifecycle reuses mission_request, mission_board, mission_workflow, mission_shared_memory, and file-artifacts rather than adding a new public MCP family.")
    (v2-item external-work-order-gate
      :status code-aligned
      :v2-source ".missiond/v2/architecture-dsl.lisp :: SSOT-before-code invariant + task contract write scope"
      :v3-pillar source-control
      :v3-function external-work-order-gate
      :surface external-work-order-gate
      :note "External Codex/ClaudeCode/user-local edits converge through the V3 work-order gate: start/verify/commit hooks require a work-order id, intent.lisp, plan.lisp, accepted_shard_id, and write-scope coverage before code is accepted.")
    (v2-item cloud-local-eventbridge
      :status code-aligned
      :v2-source ".missiond/v2/intent-event-bus.lisp :: event_router / external service event ingress"
      :v3-pillar communication
      :v3-function eventbridge
      :surface eventbridge
      :note "V2 event bus intent becomes the V3 EventBridge: local Board/slot/agent events and deploy-center/auth/timeline cloud events share missiond.event-envelope.v1, SystemEvent::ExternalServiceEvent, token-checked webhooks, idempotent durable event_log append, and mission_timeline EventBus waits.")
    (v2-item memory-provider-extraction
      :status code-aligned
      :v2-source ".missiond/v2/intent.lisp :: memory-kb / conversation-memory-distillation / ssot-retrieval-scope"
      :v3-pillar memory
      :v3-function memory-provider
      :surface memory-provider-boundary
      :note "V2 memory/KB intent (conversation archive, active memory, review overlay, skill evidence, FTS/embedding/rerank, retrieval scope) converges to the V3 memory-provider-contract: MissionD Core owns provider registry, scope resolution, query/write facades, and context-injection policy; null/local/xjp providers own data and retrieval internals. mission_kb_* tools remain compatibility leaves.")
    (v2-item eventhub-service-extraction
      :status code-aligned
      :v2-source ".missiond/v2/intent-event-bus.lisp :: cross-service durable events / waits / subscriptions"
      :v3-pillar communication
      :v3-function eventhub-service
      :surface eventhub-service-boundary
      :note "V2 cross-service event intent converges to the V3 eventhub-service-contract: xjp-eventhub owns durable streams, cursors, subscriptions, waits, dead-letter, and replay; MissionD keeps a local low-latency EventBus authoritative for agent/Board/slot/workflow wakeups with an outbound spool for cross-service relay so local orchestration stays offline-safe.")
    (v2-item review-gate-policy
      :status code-aligned
      :v2-source ".missiond/v2/intent.lisp :: review-gate-policy / review-gate-resolution-v0"
      :v3-pillar review
      :v3-function review-gate
      :surface review-gate
      :note "V2 review policy and explicit resolution bridge map to the V3 review-gate function.")
    (v2-item productive-task-runner-loop
      :status code-aligned
      :v2-source ".missiond/v2/intent.lisp :: productive-task-runner-loop"
      :v3-pillar task-runner
      :v3-function task-runner-lifecycle
      :surface task-runner-cli
      :note "V2 wave/task lifecycle, receipts, reports, and parent hotfix finalization are the V3 task-runner surface.")
    (v2-item source-hygiene-and-task-scope
      :status code-aligned
      :v2-source ".missiond/v2/architecture-dsl.lisp :: R013/R014 scoped commit rules"
      :v3-pillar source-control
      :v3-function source-hygiene
      :surface source-hygiene
      :note "V2 source hygiene rules are executable in staged/source guard scripts and task-scope guard.")
    (v2-item lisp-code-drift-governance
      :status code-aligned
      :v2-source ".missiond/v2/architecture-dsl.lisp :: SSOT-before-code invariant + task contract write scope"
      :v3-pillar source-control
      :v3-function lisp-code-drift
      :surface lisp-code-drift-policy
      :note "V2 SSOT discipline is made explicit as the V3 direct-code-drift policy: ordinary code changes need a Lisp/checker归宿, while emergency fixes must create a backfill BoardTask.")
    (v2-item commit-lisp-convergence-after-commit
      :status runtime-projected
      :v2-source ".missiond/v2/intent-worker.lisp :: ContextualCommitDetected / lisp_survey backfill"
      :v3-pillar source-control
      :v3-function commit-lisp-convergence
      :surface commit-lisp-convergence-loop
      :note "V2 commit-triggered Lisp survey is narrowed into a workflow-backed convergence loop: commit events inspect committed snapshots with git diff-tree, classify Lisp/checker/evidence coverage, and create visible backfill BoardTasks for code-only commits.")
    (v2-item lisp-code-sync-after-ssot-edit
      :status runtime-projected
      :v2-source ".missiond/v2/architecture-dsl.lisp :: SSOT-before-code invariant / worker backfill expectation"
      :v3-pillar source-control
      :v3-function lisp-code-sync
      :surface lisp-code-sync-loop
      :note "The SSOT-before-code invariant is now event-driven for Lisp edits: .missiond Lisp/checker changes emit SystemEvent::ConfigChanged, run typed compile plus code-isomorphism gates, write lisp-code-sync reports, and create visible exact-shard BoardTasks when code falls behind Lisp.")
    (v2-item lisp-code-sync-storm-circuit
      :status runtime-projected
      :v2-source ".missiond/v2/architecture-dsl.lisp :: SSOT-before-code invariant / worker backfill expectation"
      :v3-pillar source-control
      :v3-function same-source-storm-circuit-breaker
      :surface lisp-code-sync-storm-circuit
      :note "Repeated same-source Lisp/code sync failures converge into the V3 storm circuit so MissionD creates one deduped diagnostic task instead of recursively fanning out sync BoardTasks.")
    (v2-item context-pack-two-stage-parallel-work
      :status runtime-projected
      :v2-source ".missiond/v2/intent.lisp :: task-runner loop evidence + wave29 context-pack upgrade"
      :v3-pillar coordination
      :v3-function context-pack
      :surface context-pack
      :note "Shared-memory append practice is lifted into the V3 two-stage context-pack surface; mapped integration-plan dispatch groups now project through scripts/context-pack-materialize-wave.mjs and scripts/context-pack-run-wave.mjs into prepared manifest/task-contract worker shards using V3 workstation-config model_profile and timeout defaults.")
    (v2-item mission-board-coordination
      :status code-aligned
      :v2-source ".missiond/v2/intent-flow.lisp :: board-task-main-lifecycle"
      :v3-pillar coordination
      :v3-function mission-board
      :surface mission_board
      :note "V2 board task lifecycle and claim mechanics converge into mission_board.")
    (v2-item board-search-noise-governance
      :status code-aligned
      :v2-source ".missiond/v2/intent-flow.lisp :: board-task-main-lifecycle"
      :v3-pillar coordination
      :v3-function board-search-noise-governance
      :surface board-search-noise-governance
      :note "Board search defaults converge into a V3 noise-governance surface: active task scope is default, historical done/skipped search is opt-in, and cleanup candidate queries remain explicit.")
    (v2-item claudecode-workstation-config
      :status runtime-projected
      :v2-source ".missiond/v2/intent-worker.lisp :: claudecode-workstation-orchestration"
      :v3-pillar workstation
      :v3-function workstation-config
      :surface workstation-config
      :note "V2 workstation policy now has explicit V3 model/profile, timeout, prompt, ownership, and close-owner contracts; mission_task_delegate reads V3 workstation-config through WorkstationRuntimeConfig::load_for_project_root for model-profile and timeout projection.")
    (v2-item workstation-pool-unified-compute
      :status runtime-projected
      :v2-source ".missiond/v2/intent-worker.lisp :: claudecode-workstation-orchestration + wave29/30/31 multi-agent dispatch evidence"
      :v3-pillar workstation
      :v3-function workstation-pool
      :surface workstation-pool
      :note "V2 workstation orchestration and later multi-agent dispatch evidence converge into the compact V3 workstation-pool: Claude Code Default and Gemini CLI are declared once in Lisp, projected into SlotManager/MissionControl runtime slots, selected by Autopilot for unassigned BoardTasks, and exposed by mission_compute_slot list.")
    (v2-item resident-codex-master-control
      :status runtime-projected
      :v2-source ".missiond/v2/intent-event-bus.lisp :: event_router + user brain/neural-eventbus design notes"
      :v3-pillar workstation
      :v3-function resident-master-control
      :surface resident-master-control
      :note "The event-bus nervous-system philosophy now has a resident Codex master-control lane: GPT-5.5 xhigh reads Board/KB/Lisp/events, checkpoints decisions, and dispatches Claude/Gemini/Codex workers through durable BoardTask/Autopilot events.")
    (v2-item nightly-evolution-self-review
      :status runtime-projected
      :v2-source ".missiond/v2/intent-worker.lisp :: architecture maintenance / resident lisp review"
      :v3-pillar workstation
      :v3-function nightly-evolution
      :surface nightly-evolution-loop
      :note "V2 architecture maintenance becomes a conservative workflow.lisp muscle memory: nightly evolution now defaults to MissionD V3 active-authoring SSOT plus compiled semantic/workflow projections only. It runs scripts/analyze-v3-self-evolution.mjs --json over the V3 blueprint, compiled-semantic-ir, compiled-workflows, V3 checker output, and final convergence static snapshot, then writes report/proposal artifacts; apply=true may create one visible non-auto-executing review BoardTask. KB, historical conversations, provider logs, worker telemetry, Board open-task evidence, and recent commit history belong to later explicit workflows.")
    (v2-item event-driven-autopilot-runtime
      :status code-aligned
      :v2-source ".missiond/v2/intent-event-bus.lisp :: event_router / BoardTaskCreated / SlotBecameIdle"
      :v3-pillar workstation
      :v3-function delegated-boardtask-runtime
      :surface autopilot-runtime
      :note "V2 event-router intent is promoted into V3 Autopilot runtime: BoardEvent and SlotEvent subscribers wake delegated BoardTask dispatch through the event bus, preserving the existing dedicated Autopilot task as the pty.send owner.")
    (v2-item workstation-dispatch-substrate
      :status code-aligned
      :v2-source ".missiond/v2/intent.lisp :: workstation-dispatch-v0"
      :v3-pillar workstation
      :v3-function workstation-dispatch
      :surface workstation-dispatch
      :note "V2 workstation dispatch opt-in/auto-inference becomes the V3 dispatch substrate.")
    (v2-item ops-infra-deploy-format
      :status code-aligned
      :v2-source ".missiond/v2/intent-system-layer.lisp :: infra/deploy/governance"
      :v3-pillar operations
      :v3-function ops-infra
      :surface ops-infra
      :note "Local deploy, smoke, and scoped formatting are code-aligned V3 operations.")
    (v2-item runtime-load-explanation
      :status code-aligned
      :v2-source ".missiond/v2/intent-system-layer.lisp :: infra/deploy/governance"
      :v3-pillar operations
      :v3-function runtime-load-explanation
      :surface runtime-load-explanation
      :note "Runtime-load explanation converges V2 operational diagnostics into one V3 read surface that attributes daemon-internal load across lisp-code-sync, EventBus, shared-memory/workflow-runner, context-prefetch, Autopilot/DB, and nightly evolution.")
    (v2-item missiond-blue-green-self-update
      :status code-aligned
      :v2-source ".missiond/v2/intent-system-layer.lisp :: infra/deploy/governance"
      :v3-pillar operations
      :v3-function missiond-blue-green-self-update
      :surface missiond-blue-green-self-update
      :note "MissionD self-update converges into a V3 blue-green release surface: typed Lisp runtime projection, immutable release directories, active symlink switch, smoke gates, rollback, and cleanup are all code-aligned.")
    (v2-item knowledge-memory-and-kb
      :status runtime-projected
      :v2-source ".missiond/v2/intent.lisp :: memory/kb-manager"
      :v3-pillar memory
      :v3-function knowledge-memory
      :surface memory-kb
      :note "KB, beacon, memory, insight, and intent snapshot tools are physically split under the V3 memory-kb surface; memory-kb-policy now projects realtime memory pending batch size and preview truncation budgets into mission_memory runtime.")
    (v2-item codex-boot-context
      :status code-aligned
      :v2-source ".missiond/v2/intent-worker.lisp :: claudecode-workstation-orchestration"
      :v3-pillar memory
      :v3-function codex-boot-context
      :surface codex-boot-context
      :note "Codex boot context converges worker startup and external-chat handoff into a compact V3 context capsule so agents receive shared collaboration protocol and task grounding before full Lisp.")
    (v2-item durable-shared-memory
      :status code-aligned
      :v2-source ".missiond/tasks/schema/shared-memory-v1.lisp :: shared-memory-schema missiond.shared-memory.v1 (compatibility projection)"
      :v3-pillar memory
      :v3-function mission-shared-memory
      :surface mission-shared-memory
      :note "Durable concurrent-agent coordination substrate: Rust SharedMemoryService + Postgres shared_events / shared_artifacts / shared_claims / agent_cursors, surfaced as mission_shared_memory / mission_context_slice / mission_claim_status. The legacy v1 ledger remains as a file-level compatibility projection only; the durable runtime owns concurrent write authority.")
    (v2-item evidence-governance-view
      :status code-aligned
      :v2-source ".missiond/v2/intent-worker.lisp :: memory/timeline/conversation/board read models (evidence governance unification)"
      :v3-pillar memory
      :v3-function evidence-governance-view
      :surface evidence-governance-view
      :note "Unified evidence read surface served via mission_shared_memory(action=evidence_view); enforces a single authority order across task_result_artifacts (canonical worker output), conversations (provider/user read model), event_log/shared_events (causality), KB with knowledge_review_state (reviewed long-term knowledge), and BoardTask state (coordination projection).")
    (v2-item project-registry
      :status runtime-projected
      :v2-source ".missiond/v2/intent-worker.lisp :: project-root-spawn-cwd / ProjectRegistry"
      :v3-pillar memory
      :v3-function project-registry
      :surface project-registry
      :note "Project root resolution and registry behavior are physically split and pinned under the V3 project-registry surface; project-registry-policy now projects intent-path candidates and default universe manifest into mission_project init/import_universe/survey runtime.")
    (v2-item data-residency-universe
      :status code-aligned
      :v2-source ".missiond/v2/intent-worker.lisp :: project registry"
      :v3-pillar memory
      :v3-function data-residency-universe
      :surface data-residency-universe
      :note "Data-bearing project M6 gate is extended from registry-only to region-aware: cn/global hard partitions plus global-eu operating zone are pinned under data-residency-universe. The active abstraction is XJP platform partition first: xjp-cn on Aliyun ECS and xjp-global on GCP VM own auth/secret/payment/storage/router/event/deploy boundaries, while PCEA/CUTHUB bind their app partitions to those platform stacks. PCEA's project-local intent.lisp and check.sh mirror the same declarations so data-residency is a checked SSOT, not free-form deployment notes.")
    (v2-item board-frontend-project-blueprint
      :status code-aligned
      :v2-source ".missiond/v2/architecture-dsl.lisp :: lisp-code-isomorphism"
      :v3-pillar memory
      :v3-function board-frontend
      :surface board-frontend
      :note "The V2 design philosophy that implementation must mirror Lisp is extended to the Board frontend through a project-local blueprint registered from V3; this keeps backend V3 compact while creating the reusable 20+ project pattern.")
    (v2-item conversation-ingestion
      :status runtime-projected
      :v2-source ".missiond/v2/intent-worker.lisp :: conversation-jsonl-ingest / session organizer"
      :v3-pillar communication
      :v3-function conversation-ingestion
      :surface conversation-ingestion
      :note "Conversation query, analysis, timeline, retrospective, embedding, and reconcile tools are physically split and pinned under the V3 conversation-ingestion surface; conversation-ingestion-policy now projects read-model default and max limits into conversation, event, and timeline runtime.")
    (v2-item router-policy-dry-run-chain
      :status runtime-projected
      :v2-source ".missiond/v2/intent.lisp :: router-policy-v1 / router-backend-readiness-loop / router-dispatch-descriptor-loop"
      :v3-pillar communication
      :v3-function router-policy
      :surface router-policy
      :note "Router chat runtime, flow/Sonnet gateway defaults, and mission_plan advisory router-policy dry-run chain are physically split and pinned under the V3 router-policy surface; router-runtime-policy projects model/token/timeout/compression budgets into Rust.")
    (v2-item question-incident-governance
      :status code-aligned
      :v2-source ".missiond/v2/intent-worker.lisp :: system-support/incidents + question flow"
      :v3-pillar communication
      :v3-function incident-question-governance
      :surface incident-governance
      :note "Question CRUD, decision stats, LLM trace, Gemini auth, and incident routing are physically split and pinned under the V3 incident-governance surface.")
    (v2-item decision-inbox-revalidation
      :status code-aligned
      :v2-source ".missiond/v2/intent-flow.lisp :: review/decision queue"
      :v3-pillar communication
      :v3-function decision-inbox-revalidation
      :surface decision-inbox-revalidation
      :note "Decision inbox revalidation converges stale question handling into a V3 communication surface: mission_question list/get re-checks current runtime evidence, resolves obsolete lisp-code-sync incident questions, closes linked stale BoardTasks, and returns revalidation status before asking the user.")
    (v2-item capability-governance
      :status runtime-projected
      :v2-source ".missiond/v2/intent-capability-governance.lisp"
      :v3-pillar communication
      :v3-function capability-governance
      :surface capability-governance
      :note "Capability usage, audit, and Codex ops are physically pinned under the V3 capability-governance surface; capability-governance-policy now projects review sidecar location plus protected source/target lists into mission_capability_usage runtime.")
    (v2-item compute-primitives
      :status runtime-projected
      :v2-source ".missiond/v2/intent-worker.lisp :: pty / llm / worker / engine runtime"
      :v3-pillar worker-runtime
      :v3-function compute-primitives
      :surface compute-primitives
      :note "Task, PTY, job, flow_run, process, CC, forge, slot, pause, and worker primitives are physically pinned under the V3 compute-primitives surface; flow-runtime-policy projects missing FlowDefinition node defaults; compute-runtime-policy projects low-level tracked PTY spawn timeouts; mission_compute_slot and mission_task_delegate remain owned by workstation-config.")
    (v2-item skill-runtime
      :status code-aligned
      :v2-source ".missiond/v2/intent-worker.lisp :: skill workflow executor"
      :v3-pillar worker-runtime
      :v3-function skill-runtime
      :surface skill-runtime
      :note "Skill query/context/mutate/exec is physically split and pinned under the V3 skill-runtime surface.")
    (v2-item cascade-universe-governance
      :status runtime-projected
      :v2-source ".missiond/v2/intent-event-bus.lisp :: cascade/control tree"
      :v3-pillar worker-runtime
      :v3-function cascade-governance
      :surface cascade-governance
      :note "Universe graph, cascade planning, trigger execution, and integrity linting are physically split and pinned under the V3 cascade-governance surface; cascade-policy now projects default manifest, allowed root, trigger switch, and max-cycle bounds into cascade path/trigger runtime.")
    (v2-item sysinfra-control
      :status code-aligned
      :v2-source ".missiond/v2/intent-system-layer.lisp :: system/sysinfra tools"
      :v3-pillar operations
      :v3-function sysinfra-control
      :surface sysinfra-control
      :note "Infra query/ops, permission query/mutate, power control, system logs/config/update, and global instruction tools are physically pinned under the V3 sysinfra-control surface.")

    (public-surface-map
      :source-scan "crates/missiond-mcp/src/tools/**/*.rs"
      :rule "Every ToolDefinition::new public MCP tool must appear in exactly one tool-group; code-aligned groups must point only at code-aligned implementation surfaces."
      (tool-group request-entry
        :status code-aligned
        :v2-source ".missiond/v2/intent.lisp :: unified-entry-pipeline"
        :v3-pillar request
        :v3-function request-lifecycle
        :surface mission_request
        :tools [mission_request])
      (tool-group directive-entry
        :status code-aligned
        :v2-source ".missiond/v2/intent.lisp :: directive-layer"
        :v3-pillar intent
        :v3-function directive-authoring
        :surface mission_directive
        :tools [mission_directive])
      (tool-group plan-entry
        :status code-aligned
        :v2-source ".missiond/v2/intent.lisp :: plan-runner"
        :v3-pillar plan
        :v3-function plan-authoring-and-runner
        :surface mission_plan
        :tools [mission_plan])
      (tool-group workflow-entry
        :status code-aligned
        :v2-source ".missiond/v2/intent.lisp :: workflow"
        :v3-pillar workflow
        :v3-function workflow-distillation
        :surface mission_workflow
        :tools [mission_workflow])
      (tool-group execution-entry
        :status code-aligned
        :v2-source ".missiond/v2/intent-execution-governance.lisp"
        :v3-pillar execution
        :v3-functions [execution-log execution-claim-lease execution-completion-audit]
        :surfaces [mission_execution-log mission_execution-claim-lease mission_execution-completion-audit]
        :tools [mission_execution])
      (tool-group board-entry
        :status code-aligned
        :v2-source ".missiond/v2/intent-flow.lisp :: board-task-main-lifecycle"
        :v3-pillar coordination
        :v3-function mission-board
        :surface mission_board
        :tools [mission_board_query mission_board_create mission_board_update mission_board_delete
                mission_board_claim mission_board_note_add mission_board_decompose mission_board_retry
                mission_submit_phase_result])
      (tool-group workstation-entry
        :status runtime-projected
        :v2-source ".missiond/v2/intent-worker.lisp :: claudecode-workstation-orchestration"
        :v3-pillar workstation
        :v3-function workstation-config
        :surface workstation-config
        :tools [mission_compute_slot mission_task_delegate])
      (tool-group resident-master-entry
        :status code-aligned
        :v2-source ".missiond/v2/intent-worker.lisp :: orchestration brain/checkpoint"
        :v3-pillar workstation
        :v3-function resident-master-control
        :surface resident-master-control
        :tools [mission_master_status mission_convergence_status mission_nightly_evolution mission_swarm_run])
      (tool-group compute-runtime-tools
        :status code-aligned
        :v2-source ".missiond/v2/intent-worker.lisp :: worker path runtime primitives"
        :v3-pillar worker-runtime
        :v3-function compute-primitives
        :surface compute-primitives
        :tools [mission_task_submit mission_task_query mission_task_cancel mission_job_poll mission_flow_run
                mission_pty_spawn mission_pty_send mission_pty_read mission_pty_signal mission_pty_confirm
                mission_pty_status mission_pty_screenshot mission_slots mission_slot_history mission_agent
                mission_inbox mission_sonnet_process mission_minimax_process mission_cc_query mission_cc_swarm
                mission_worker mission_control mission_pause mission_forge_build mission_forge_lint])
      (tool-group knowledge-memory-tools
        :status code-aligned
        :v2-source ".missiond/v2/intent.lisp :: memory/kb-manager"
        :v3-pillar memory
        :v3-function knowledge-memory
        :surface memory-kb
        :tools [mission_kb_query mission_kb_remember mission_kb_mutate mission_kb_review mission_kb_ops mission_beacon
                mission_code_search mission_memory mission_insight mission_intent mission_context_gather mission_context_boot])
      (tool-group shared-memory-tools
        :status code-aligned
        :v2-source ".missiond/tasks/schema/shared-memory-v1.lisp :: shared-memory-schema missiond.shared-memory.v1 (compatibility projection)"
        :v3-pillar memory
        :v3-function mission-shared-memory
        :surface mission-shared-memory
        :tools [mission_shared_memory mission_context_slice mission_claim_status])
      (tool-group project-registry-tools
        :status code-aligned
        :v2-source ".missiond/v2/intent-worker.lisp :: project registry"
        :v3-pillar memory
        :v3-function project-registry
        :surface project-registry
        :tools [mission_project])
      (tool-group skill-runtime-tools
        :status code-aligned
        :v2-source ".missiond/v2/intent-worker.lisp :: skill workflow executor"
        :v3-pillar worker-runtime
        :v3-function skill-runtime
        :surface skill-runtime
        :tools [mission_skill_query mission_skill_context mission_skill_mutate mission_skill_exec])
      (tool-group cascade-runtime-tools
        :status code-aligned
        :v2-source ".missiond/v2/intent-event-bus.lisp :: cascade"
        :v3-pillar worker-runtime
        :v3-function cascade-governance
        :surface cascade-governance
        :tools [mission_universe_graph mission_cascade_plan mission_cascade_trigger mission_cascade_lint])
      (tool-group conversation-ingestion-tools
        :status code-aligned
        :v2-source ".missiond/v2/intent-worker.lisp :: conversation-jsonl-ingest"
        :v3-pillar communication
        :v3-function conversation-ingestion
        :surface conversation-ingestion
        :tools [mission_conversation_query mission_conversation_analyze mission_conversation_reconcile
                mission_timeline mission_retrospective_manage mission_embedding_ops])
      (tool-group router-policy-tools
        :status runtime-projected
        :v2-source ".missiond/v2/intent.lisp :: router-policy-v1"
        :v3-pillar communication
        :v3-function router-policy
        :surface router-policy
        :tools [mission_router_chat mission_router_chat_manage])
      (tool-group question-incident-tools
        :status code-aligned
        :v2-source ".missiond/v2/intent-worker.lisp :: incident/question"
        :v3-pillar communication
        :v3-function incident-question-governance
        :surface incident-governance
        :tools [mission_question mission_llm_trace mission_decision_stats mission_gemini_auth mission_incident])
      (tool-group interaction-gateway-tools
        :status code-aligned
        :v2-source ".missiond/v2/intent.lisp :: Jarvis/web/mobile/external user entry"
        :v3-pillar communication
        :v3-function interaction-gateway
        :surface interaction-gateway
        :tools [mission_interaction]
        :note "mission_interaction is the consolidated MCP facade for external channel receive/confirm/follow/status. HTTP adapters for Web/iOS/Jarvis/WeChat normalize to InteractionEnvelope and must not directly write provider PTYs or bypass grounded intent/plan gates.")
      (tool-group capability-audit-tools
        :status code-aligned
        :v2-source ".missiond/v2/intent-capability-governance.lisp"
        :v3-pillar communication
        :v3-function capability-governance
        :surface capability-governance
        :tools [mission_capability_usage mission_audit mission_codex_ops mission_codex_replay])
      (tool-group mcp-tool-governance-tools
        :status code-aligned
        :v2-source ".missiond/v2/intent-capability-governance.lisp :: tool-directory"
        :v3-pillar communication
        :v3-function capability-governance
        :surface capability-governance
        :tools [mission_tool_directory mission_agent_navigation])
      (tool-group sysinfra-control-tools
        :status code-aligned
        :v2-source ".missiond/v2/intent-system-layer.lisp :: sysinfra"
        :v3-pillar operations
        :v3-function sysinfra-control
        :surface sysinfra-control
        :tools [mission_infra_query mission_infra_ops mission_permission_query mission_permission_mutate
                mission_power_control mission_sys_logs mission_sys_config mission_daemon_update
                mission_global_instruction])))
