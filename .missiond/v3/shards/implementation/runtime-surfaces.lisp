(implementation-map
(surface commit-lisp-convergence-loop
      :status "code-aligned"
      :implements [commit-lisp-convergence commit-lisp-convergence-loop]
      :code [".missiond/v3/missiond-blueprint.lisp"
             ".missiond/workflows/commit-lisp-convergence.lisp"
             "crates/missiond-daemon/src/engine/commit_convergence.rs"
             "crates/missiond-daemon/src/engine/mod.rs"
             "crates/missiond-daemon/src/main.rs"
             "crates/missiond-daemon/src/engine/master_control.rs"
             "scripts/check-v3-commit-convergence-loop.mjs"
             "scripts/check-v3-code-isomorphism-complete.mjs"]
      :note "commit-lisp-convergence-loop is the event-driven code->Lisp backfill muscle. CommitConvergenceService subscribes to SystemEvent::ContextualCommitDetected, resolves project from the committing slot, provider conversation project/project_id metadata, or registry, inspects committed snapshots with git diff-tree --root --no-commit-id -r --name-only <sha>, classifies code/lisp/checker/evidence/doc files, writes commit convergence reports, and creates one visible deduped BoardTask commit-lisp-backfill:<project>:<sha> for code-only commits. Commits mentioned by provider logs but absent from all registered local roots are external-or-unavailable-commit diagnostics, not unknown-project registry defects. Lisp/checker/evidence-only commits do not recurse.")

(surface lisp-code-sync-loop
      :status "code-aligned"
      :implements [lisp-code-sync lisp-code-sync-loop]
      :code [".missiond/v3/missiond-blueprint.lisp"
             ".missiond/workflows/lisp-code-sync.lisp"
             "crates/missiond-daemon/src/engine/lisp_code_sync.rs"
             "crates/missiond-daemon/src/engine/intent_engine/autopilot.rs"
             "crates/missiond-daemon/src/engine/mod.rs"
             "crates/missiond-daemon/src/main.rs"
             "crates/missiond-daemon/src/engine/master_control.rs"
             "scripts/check-v3-lisp-code-sync-isomorphism.mjs"
             "scripts/check-v3-code-isomorphism-complete.mjs"]
      :note "lisp-code-sync-loop watches active ProjectRegistry .missiond authoring paths, emits SystemEvent::ConfigChanged, ignores .missiond/v3/runtime/** plus cold evidence, suppresses unchanged fingerprints, and keeps the subscriber enqueue-only through the durable reconciliation queue in lisp_code_sync_jobs for queued ConfigChanged events. The reconciler claims due jobs with lease, runs typed compile plus code-isomorphism gates, writes bounded reports, exposes queue metrics/reportDirs/stormCircuitHits/recentSyncTaskCreations, creates or reuses one deduped BoardTask for failing gates, switches to lisp-code-sync:<project>:storm-circuit on storms, and lets Autopilot close stale runtime-report tasks before slot selection. It never edits code directly; mutation still requires evidence-plan, accepted shard, write_scope, acceptance, and durable green gates.")

(surface lisp-code-sync-storm-circuit
      :status "code-aligned"
      :implements [same-source-storm-circuit-breaker lisp-code-sync-storm-circuit]
      :code [".missiond/v3/missiond-blueprint.lisp"
             ".missiond/workflows/lisp-code-sync.lisp"
             "crates/missiond-daemon/src/engine/lisp_code_sync.rs"
             "crates/missiond-daemon/src/engine/master_control.rs"
             "scripts/check-v3-lisp-code-sync-isomorphism.mjs"]
      :note "lisp-code-sync-storm-circuit is the runtime governance surface for same-source sync storms. It counts recent sync BoardTask creations, switches from timestamp/path-hash task identity to semantic root_cause_key lisp-code-sync:<project>:storm-circuit, reuses one visible root-cause task while the circuit is active, appends further evidence through reports/status, and exposes stormCircuitHits/recentSyncTaskCreations/reportDirs through mission_master_status. This prevents one runtime self-output loop from spawning one worker per report path.")

(surface nightly-evolution-loop
      :status "code-aligned"
      :implements [nightly-evolution night-scheduler nightly-evolution-loop]
      :code [".missiond/v3/missiond-blueprint.lisp"
             ".missiond/workflows/nightly-evolution.lisp"
             "crates/missiond-daemon/src/engine/nightly_evolution.rs"
             "crates/missiond-daemon/src/engine/mod.rs"
             "crates/missiond-daemon/src/main.rs"
             "crates/missiond-daemon/src/engine/master_control.rs"
             "crates/missiond-daemon/src/handlers/compute/slot.rs"
             "crates/missiond-mcp/src/tools/compute/process.rs"
             "crates/missiond-mcp/src/gen_gateway.rs"
             "scripts/analyze-v3-self-evolution.mjs"
             "scripts/check-v3-nightly-evolution-isomorphism.mjs"
             "scripts/check-v3-code-isomorphism-complete.mjs"]
      :note "nightly-evolution-loop turns resident master self-review into a reusable proposal-only workflow. NightlyEvolutionService is manual-first: scheduled periodic runs are disabled by default while active supervision and external worker sessions are running, and require MISSIOND_NIGHTLY_EVOLUTION_SCHEDULE=true. mission_nightly_evolution can manually run the same workflow. Its default evidence set is deliberately narrow: MissionD V3 active-authoring Lisp, compiled-semantic-ir, compiled-workflows, V3 checker output, and final convergence static snapshot. It does not read KB, historical conversations, provider logs, worker telemetry, Board open tasks, or recent commit history unless a later explicit workflow asks for them. The report writes .missiond/v3/runtime/nightly-evolution/<date>.report.lisp, writes at most three .missiond/v3/runtime/self-evolution/<timestamp>-<finding_id>.proposal.lisp artifacts, and only creates one visible review BoardTask with auto_execute=false when apply=true and risk gates allow it.")

(surface workstation-config
      :status "code-aligned"
      :implements [workstation-config]
      :code ["crates/missiond-daemon/src/handlers/compute/compute_slot.rs"
             "crates/missiond-daemon/src/handlers/compute/task_delegate.rs"
             "crates/missiond-daemon/src/main.rs"
             "crates/missiond-daemon/src/context/slot_env.rs"
             "crates/missiond-daemon/src/context/v3_blueprint_runtime.rs"
             "crates/missiond-daemon/src/slot_orchestrator/spawner.rs"
             "crates/missiond-daemon/src/slot_orchestrator/cc_controller.rs"
             "crates/missiond-daemon/src/llm/gemini_driver.rs"
             "crates/missiond-daemon/src/engine/intent_engine/autopilot.rs"
             "crates/missiond-mcp/src/tools/compute/compute_slot.rs"
             "crates/missiond-mcp/src/tools/compute/task_delegate.rs"]
	    :note "mission_compute_slot and mission_task_delegate accept model/model_profile; coder/researcher default to Claude Code Default(Opus 4.7/1M) by omitting --model. mission_task_delegate accepts two-stage delegation metadata, writes canonical task_contracts, creates subject-bound worker/conversation capability_grants only when the concrete subject is known, projects sandbox_profile, and mirrors facts into BoardTask.runtime_metadata only as UI/cache projection. BoardTask description is a prompt projection and is never parsed by runtime control paths. scope_semantics separates readable evidence from writable scope and keeps must_not_touch as a write/stage/commit ban. main.rs startup SlotManager registration loads WorkstationRuntimeConfig and startup-slot entries; ClaudeCode startup slots project model_profile through spawn_model_for_profile."
	    :evidence-sidecar ".missiond/v3/evidence/blueprint-notes.lisp#note-012")

(surface workstation-pool
      :status "code-aligned"
      :implements [workstation-pool]
      :code ["crates/missiond-daemon/src/context/v3_blueprint_runtime.rs"
             "crates/missiond-daemon/src/main.rs"
             "crates/missiond-daemon/src/slot_orchestrator/generic_cli.rs"
             "crates/missiond-pty/src/session.rs"
             "crates/missiond-pty/src/pty_recognition.rs"
             "crates/missiond-pty/src/manager.rs"
             "crates/missiond-core/src/types/slot.rs"
             "crates/missiond-core/src/types/project.rs"
             "crates/missiond-daemon/src/engine/intent_engine/autopilot.rs"
             "crates/missiond-daemon/src/handlers/compute/compute_slot.rs"
             "crates/missiond-daemon/src/handlers/compute/slot.rs"
             "crates/missiond-core/src/core/slot_manager.rs"
             "scripts/check-v3-workstation-pool-isomorphism.mjs"]
      :evidence ".missiond/v3/evidence/workstation-pool.lisp"
      :note "workstation-pool is the compact V3 compute-account SSOT. It declares ClaudeCode Opus/Sonnet lanes, Gemini legacy read-only lanes, Agy successor research lanes, Codex code/review worker lanes, and the non-shard Codex master lane; runtime projection feeds SlotManager, PTYSpawnOptions, Autopilot routing, mission_compute_slot list, mission_slots legacy-Sonnet filtering, and public /api/slots slot telemetry. mission_slots and /api/slots MUST project activeBoardTaskId/currentTaskId and activeBoardTask by joining running BoardTasks on assignee or pty_slot claim so the Board cockpit and Terminal tab can show what each visible PTY is actually doing."
      :evidence-sidecar ".missiond/v3/evidence/workstation-pool.lisp")

(surface resident-master-control
      :status "code-aligned"
      :implements [resident-master-control master-checkpoint master-event-subscriber master-decision-loop master-delegation master-recovery night-scheduler commit-lisp-convergence-loop nightly-evolution-loop]
      :code [".missiond/v3/missiond-blueprint.lisp"
             "crates/missiond-daemon/src/context/v3_blueprint_runtime.rs"
             "crates/missiond-daemon/src/engine/master_control.rs"
             "crates/missiond-daemon/src/engine/commit_convergence.rs"
             "crates/missiond-daemon/src/engine/nightly_evolution.rs"
             "crates/missiond-daemon/src/main.rs"
             "crates/missiond-daemon/src/handlers/compute/slot.rs"
             "crates/missiond-pty/src/session.rs"
             "crates/missiond-core/src/types/slot.rs"
             "scripts/check-v3-master-control-isomorphism.mjs"]
      :note "resident-master-control promotes Codex to a non-shard orchestrator. Runtime projection starts GPT-5.5 xhigh read-only Codex, writes phaseful checkpoints, exposes mission_master_status and mission_convergence_status, supervises commit-lisp-convergence-loop and manual-first nightly-evolution-loop status, and keeps provider logs as completion authority while PTY remains diagnostic. mission_convergence_status also exposes activeRelease from the blue-green release manifest, including typed_lisp_runtime projection completeness, so operators can see whether the running release carries compiled V3/universe/workflow snapshots. The resident master does not perform autonomous self-review from heartbeat/SlotEvent noise: no active_objective_id means no-op. Active BoardTask objectives are the load-bearing objective; if the master says it will create/update a BoardTask, it must perform the Board MCP mutation before final response. Master context-pack paths are projected from the resident slot project_root so launchd cwd=/ cannot produce invalid /.missiond context paths.")

(surface autopilot-runtime
      :status "code-aligned"
      :implements [delegated-boardtask-runtime event-driven-autopilot-handoff]
      :code ["crates/missiond-daemon/src/bus/v2_subscribers.rs"
             "crates/missiond-daemon/src/engine/intent_engine/autopilot.rs"
             "crates/missiond-daemon/src/engine/intent_engine/flow_engine.rs"
             "crates/missiond-daemon/src/handlers/compute/task_delegate.rs"
             "crates/missiond-daemon/src/handlers/knowledge/board/events.rs"
             "crates/missiond-core/src/event/events/board.rs"
             "crates/missiond-core/src/event/events/slot.rs"
             "scripts/check-v3-autopilot-runtime-isomorphism.mjs"]
      :note "autopilot-runtime is the event-driven muscle layer for delegated BoardTasks. task_delegate and mission_board_create publish BoardEvent::TaskCreated; v2_subscribers owns v2_autopilot_board_event and v2_autopilot_slot_event, which wake board_dispatch_notify on BoardEvent::TaskCreated, reopened BoardEvent status updates, and SlotEvent::BecameIdle, then ack without running pty.send inline. The dedicated Autopilot task remains the only prompt/close owner: it claims eligible open BoardTasks, derives leases/timeouts from V3 policy, holds a per-slot dispatch guard across state.pty.send, emits SlotEvent::TaskDispatched, and records PTY/provider outputs as observation or task_result_candidate only. status=done transitions MUST flow through worker_settle or equivalent typed settle with a completed task-result-artifact hash; Autopilot MUST NOT synthesize canonical completion from PTY/provider prose, Board notes, or Markdown description. This preserves the event-bus causal chain while keeping long-running worker interaction outside subscriber ack paths.")

(surface genome-runtime
      :status "code-aligned"
      :implements [genome-compiler atom-registry cell-runtime tissue-profile autopilot-organ shadow-activation]
      :code [".missiond/v3/genome/autopilot.lisp"
             "tools/missiond_lispc/bin/genome_schema.ml"
             "crates/missiond-kernel/src/lib.rs"
             "crates/missiond-genome/src/lib.rs"
             "crates/missiond-organism-runtime/src/lib.rs"
             "crates/missiond-organism-runtime/src/autopilot.rs"
             "crates/missiond-daemon/src/organism/autopilot_organ.rs"
             "crates/missiond-daemon/src/bus/v2_subscribers.rs"
             "crates/missiond-daemon/src/main.rs"
             "scripts/check-v3-genome-runtime-isomorphism.mjs"
             "scripts/check-v3-autopilot-genome-isomorphism.mjs"]
      :note "genome-runtime introduces MissionD's Lisp Genome -> Rust Atom/Cell/Tissue/Organ runtime boundary. missiond-lispc validates genome Lisp and emits compiled-genomes JSON; missiond-kernel owns EventEnvelope, Effect, CommandEnvelope, AtomRegistry, Molecule, RuleGraph, Cell, TissueProfile, Genome, and ActivationMode; missiond-organism-runtime executes Cell::on_event under shadow, active, or rollback activation with budget/idempotency guards. The first migrated organ is Autopilot: board/slot subscribers run shadow parity against legacy wakeup helpers by default, active mode routes notifications/ticks/dispatch through AutopilotEffectInterpreter, and runtime errors publish incidents while falling back to the legacy path. Production activation and shadow snapshots are runtime artifacts under MISSIOND_RUNTIME_DIR/genome, while repo .missiond/v3/runtime/genome is dev/cold evidence only.")

(surface mission_board
      :status "code-aligned"
      :implements [mission-board board-task-lifecycle board-claim-lease]
      :code ["crates/missiond-daemon/src/handlers/knowledge/board.rs"
             "crates/missiond-daemon/src/handlers/knowledge/board/claim.rs"
             "crates/missiond-daemon/src/handlers/knowledge/board/create.rs"
             "crates/missiond-daemon/src/handlers/knowledge/board/decompose.rs"
             "crates/missiond-daemon/src/handlers/knowledge/board/delete.rs"
             "crates/missiond-daemon/src/handlers/knowledge/board/events.rs"
             "crates/missiond-daemon/src/handlers/knowledge/board/note.rs"
             "crates/missiond-daemon/src/handlers/knowledge/board/query.rs"
             "crates/missiond-daemon/src/handlers/knowledge/board/retry.rs"
             "crates/missiond-daemon/src/handlers/knowledge/board/session.rs"
             "crates/missiond-daemon/src/handlers/knowledge/board/update.rs"
             "crates/missiond-core/src/types/board.rs"
             "crates/missiond-core/src/db/traits.rs"
             "crates/missiond-core/src/db/pg/board.rs"
             "crates/missiond-daemon/src/engine/intent_engine/flow_engine.rs"
             "crates/missiond-daemon/src/handlers/sysinfra/misc.rs"
             "crates/missiond-daemon/src/infra/aiops.rs"
             "crates/missiond-mcp/src/tools/knowledge/board.rs"
             "scripts/check-v3-board-isomorphism.mjs"]
      :engineering-flow-gate ["mission_submit_phase_result rejects obviously short execution_plan artifacts before ConsultGemini2."
                              "ConsultGemini2 stores review evidence but advances to Execute only after an explicit approval signal; rejected or ambiguous reviews return to Plan and create a review-gate question."
                              "ConsultGemini1 remains advisory."]
      :note "mission_board is the durable BoardTask coordination projection underneath delegated worker control: MCP exposes query/create/update/delete/claim/decompose/retry/note_add with a generated schema from .missiond/intent-tools.lisp. Board handlers normalize common snake_case/camelCase aliases before schema projection, reject invalid status/noteType with structured ToolError codes, validate parentId/dependsOn before persistence, cap descriptions, reject oversized note payloads with artifact-path guidance, return compact note receipts for large stored content, and aggregate self-heal incident tasks by dedupe_key instead of auto-executing a worker per tool outage so agents recover instead of flailing on unknown errors. BoardTask claim uses typed lease semantics and CLAIM_CONFLICT; BoardTask status=done is DB-gated by task_result_artifacts/capability grants and returns typed EVIDENCE_REQUIRED or CAPABILITY_DENIED; BoardTask description and notes are projections and must not become canonical control state."
      :evidence-sidecar ".missiond/v3/evidence/blueprint-notes.lisp#note-014")

(surface board-search-noise-governance
      :status "code-aligned"
      :implements [board-search-noise-governance board-search-active-default historical-board-search-opt-in]
      :code ["crates/missiond-core/src/types/board.rs"
             "crates/missiond-core/src/db/pg/board.rs"
             "crates/missiond-mcp/src/tools/knowledge/board.rs"
             "crates/missiond-daemon/src/handlers/knowledge/board/query.rs"
             "scripts/check-v3-board-isomorphism.mjs"
             "scripts/check-v3-control-plane-m6-split.mjs"]
      :note "board-search-noise-governance keeps broad Board keyword searches from polluting current operational decisions with historical done/skipped tasks. mission_board_query(action=search) defaults to active statuses only; historical Board cleanup must opt in with includeHistorical=true, scope=all/historical, or an explicit done/skipped status. Responses expose meta.activeFilterApplied and meta.historicalIncluded so agents, Board UI, and cleanup workflows can explain whether historical tasks were excluded.")

(surface conversation-ingestion
      :status "code-aligned"
      :implements [conversation-ingestion timeline retrospective embedding-ops]
      :code ["crates/missiond-mcp/src/tools/comm/conversation.rs"
             "crates/missiond-mcp/src/tools/comm/timeline.rs"
             "crates/missiond-daemon/src/handlers/mod.rs"
             "crates/missiond-daemon/src/context/v3_blueprint_runtime.rs"
             "crates/missiond-daemon/src/handlers/comm/conversation.rs"
             "crates/missiond-daemon/src/handlers/comm/conversation/router.rs"
             "crates/missiond-daemon/src/handlers/comm/conversation/query.rs"
             "crates/missiond-daemon/src/handlers/comm/conversation/events.rs"
             "crates/missiond-daemon/src/handlers/comm/conversation/maintenance.rs"
             "crates/missiond-daemon/src/handlers/comm/timeline.rs"
             "crates/missiond-daemon/src/handlers/comm/retrospective.rs"
             "crates/missiond-daemon/src/context/context_pipeline.rs"
             "crates/missiond-daemon/src/workers/codex/vision_worker.rs"
             "crates/missiond-daemon/src/llm/codex_cli.rs"
             "crates/missiond-daemon/src/infra/ingestion_router.rs"
             "crates/missiond-core/src/cc_tasks/watcher.rs"
             "crates/missiond-core/src/gemini_cli/watcher.rs"
             "crates/missiond-daemon/src/workers/local/gemini_reconcile_worker.rs"
             "crates/missiond-daemon/src/workers/local/codex_ingestion_worker.rs"
             "scripts/audit-codex-history-ingestion.mjs"
             "scripts/check-v3-conversation-ingestion-isomorphism.mjs"
             "scripts/check-v3-cli-conversation-ingestion-isomorphism.mjs"]
      :note "Runtime-projected V3 destination for conversation/session/timeline/retrospective/embedding public tools. context/v3_blueprint_runtime.rs projects conversation-ingestion-policy read-model default and max limits into conversation/query.rs, conversation/events.rs, and timeline.rs, projects context prefetch intent-router model/timeout into context/context_pipeline.rs, and projects Codex vision worker binary/model/idle/absolute timeout into workers/codex/vision_worker.rs plus llm/codex_cli.rs; conversation.rs is the thin conversation-ingestion facade; conversation/router.rs owns mission_conversation_query, mission_conversation_analyze, and mission_retrospective_manage consolidated routing; conversation/query.rs owns read-model query actions including list/get/search/analysis_context/message_search/user_index/labels/context; retrospective.rs owns bulk-tool whitelist plus worker/meta threshold signalQuality so batch scans do not masquerade as reasoning waste; conversation/events.rs owns analysis/event egress including conver..."
      :evidence-sidecar ".missiond/v3/evidence/blueprint-notes.lisp#note-017")
)
