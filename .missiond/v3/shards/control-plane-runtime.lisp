  (control-plane-m6-split
    :schema "missiond.control-plane-m6-split.v1"
    :purpose "Keep MissionD's fast-growing orchestration/control plane readable by splitting overloaded policy blocks into stable subplanes while preserving existing runtime projections."
    :domains [workstation-control-plane master-control-plane eventbridge-deployment-plane eventhub-extraction-plane project-universe-plane data-residency-plane memory-access-plane knowledge-skill-plane execution-control-plane]
    (domain workstation-control-plane
      :owner workstation-config
      :source [workstation-config workstation-pool agent-interaction-policy]
      :functions [model-profile-policy slot-template-registry startup-slot-registry timeout-capacity-ttl-policy dispatch-routing-policy prompt-context-policy exact-shard-contract provider-interaction-policy]
      :runtime-projection [WorkstationRuntimeConfig workstation-pool mission_compute_slot mission_task_delegate Autopilot]
      :checker ["node scripts/check-v3-workstation-config-isomorphism.mjs" "node scripts/check-v3-workstation-pool-isomorphism.mjs" "node scripts/check-v3-control-plane-m6-split.mjs"]
      :refactor-rule "Keep concrete model/slot/timeout/prompt/fanout rules addressable by function name; do not add new dispatch invariants only as long strings inside workstation-config.")
    (domain master-control-plane
      :owner resident-master-control
      :source [resident-master-control master-checkpoint master-event-subscriber master-decision-loop master-delegation master-recovery night-scheduler commit-lisp-convergence-loop lisp-code-sync-loop nightly-evolution-loop]
      :functions [master-checkpoint master-event-intake master-objective-loop master-delegation-loop master-recovery-loop master-maintenance-loop mcp-readiness]
      :runtime-projection [MasterControlService mission_master_status mission_convergence_status master-control-checkpoint]
      :checker ["node scripts/check-v3-master-control-isomorphism.mjs" "node scripts/check-v3-control-plane-m6-split.mjs"]
      :refactor-rule "Master control is a phase machine, not a prompt blob; every new behavior must attach to one loop and state whether it wakes the resident slot, writes checkpoint only, or delegates work.")
    (domain eventbridge-deployment-plane
      :owner eventbridge
      :source [eventbridge-policy deployment-event-ingest router-usage-event-ingest deployment-change-classification-policy m6-deployment-confirmation deploy-agent-self-update-governance deployment-event-response m6-deployment-rollout]
      :functions [event-envelope-contract event-waiter-contract deployment-event-ingest router-usage-event-ingest deployment-change-classification-policy deployment-provenance-policy deploy-center-relay-contract deploy-agent-update-provenance]
      :runtime-projection [ExternalServiceEvent mission_timeline.wait deploy-center-event-webhook deployment-event-response]
      :checker ["node scripts/check-v3-eventbridge-isomorphism.mjs" "node scripts/check-v3-project-registry-isomorphism.mjs" "node scripts/check-v3-control-plane-m6-split.mjs"]
      :refactor-rule "Deployment closure uses deploy-center provenance plus smoke; CI/GitHub/curl are diagnostics unless deploy-center lacks data.")
    (domain eventhub-extraction-plane
      :owner eventhub-service-contract
      :source [eventhub-service-contract eventbridge-policy deployment-event-ingest router-usage-event-ingest]
      :functions [eventhub-service-boundary eventhub-envelope-contract local-event-spool outbound-event-relay eventhub-subscription-contract eventhub-wait-contract eventhub-dead-letter-replay eventhub-missiond-adapter]
      :runtime-projection [xjp-eventhub local-event-spool EventHubClient mission_timeline.wait ExternalServiceEvent]
      :checker ["node scripts/check-v3-service-extraction-isomorphism.mjs" "node scripts/check-v3-eventbridge-isomorphism.mjs" "node scripts/check-v3-control-plane-m6-split.mjs"]
      :refactor-rule "MissionD local EventBus remains the low-latency agent/Board/slot/workflow control bus. xjp-eventhub is the durable cross-service event backbone; MissionD syncs selected local events through an outbound spool and can continue local orchestration when xjp-eventhub is offline. Local per-topic fanout buffer MUST absorb normal startup catchup bursts (8192 events per domain as of client-channel runtime closure); slow subscribers still rewind through bootstrap with visible lag diagnostics instead of stalling publishers.")
    (domain project-universe-plane
      :owner project-registry
      :source [project-registry-policy project-identity-contract registry-authority-map project-maturity-model project-maturity-registry project-blueprint-registry service-runtime-universe data-residency-universe]
      :functions [project-identity-root-resolution registry-authority-map maturity-contract service-runtime-summary data-residency-summary deploy-fact-reference forge-catalog-reference registry-reconciliation]
      :runtime-projection [mission_project.universe compiled-project-universe project_registry_reconcile Board-System-Universe]
      :checker ["node scripts/check-project-ssot-universe.mjs" "node scripts/check-project-maturity.mjs --min-level M5" "node scripts/check-v3-control-plane-m6-split.mjs"]
      :refactor-rule "MissionD owns identity/SSOT/maturity; deploy-center owns runtime release facts; Forge owns component/pattern catalog. New project metadata must declare which authority owns it.")
    (domain data-residency-plane
      :owner data-residency-universe
      :source [data-residency-universe project-maturity-model project-blueprint-registry service-runtime-universe]
      :functions [data-region-partition-contract regional-auth-issuer-contract regional-secret-store-contract regional-storage-contract regional-payment-ledger-contract regional-router-model-policy cross-region-data-policy project-region-declaration]
      :runtime-projection [compiled-project-universe mission_project.universe Board-System-Universe project_registry_reconcile]
      :checker ["node scripts/check-v3-data-residency-universe-isomorphism.mjs" "node scripts/check-project-ssot-universe.mjs" "node scripts/check-v3-control-plane-m6-split.mjs"]
      :refactor-rule "Data-bearing projects must declare region partitions before release. cn/global are hard partitions; EU is an operating zone inside global until a project explicitly promotes it to a hard partition. Cross-region data flow is default-deny and whitelist-driven.")
    (domain memory-access-plane
      :owner memory-provider-contract
      :source [memory-provider-contract memory-kb-policy conversation-memory-distillation ssot-retrieval-scope grounding-search-aggregate]
      :functions [memory-provider-registry memory-scope-resolution memory-query-contract memory-write-contract memory-review-overlay-contract memory-export-contract memory-redaction-policy memory-context-injection-policy grounding-search-aggregate task-record-indexing]
      :runtime-projection [mission_memory mission_kb_query mission_kb_remember mission_context_gather MemoryProviderConfig context-pack-builder]
      :checker ["node scripts/check-v3-service-extraction-isomorphism.mjs" "node scripts/check-v3-memory-kb-isomorphism.mjs" "node scripts/check-v3-control-plane-m6-split.mjs"]
      :refactor-rule "MissionD Core does not own long-term memory data. It owns provider registry, scope resolution, context injection policy, and compatibility facades; provider implementations own conversation stores, active memory, review overlay, skill evidence index, FTS, embedding, rerank, export, purge, and tenant/universe/project/user isolation.")

		    (domain knowledge-skill-plane
		      :owner memory-kb
		      :source [memory-provider-contract memory-kb-policy learning-engine-policy conversation-memory-distillation skill-runtime skill-operational-fact-authority ssot-retrieval-scope skill-edit-delegation-policy]
		      :functions [skill-registry skill-search skill-project-links skill-operational-facts skill-to-workflow-promotion skill-edit-delegation-policy memory-quarantine memory-distillation memory-search-v2 provider-backed-skill-evidence]
		      :runtime-projection [mission_skill mission_skill_context.operational_facts mission_kb_query mission_kb_remember conversation-memory-distillation]
		      :checker ["node scripts/check-v3-memory-kb-isomorphism.mjs" "node scripts/check-v3-skill-runtime-isomorphism.mjs" "node scripts/check-v3-control-plane-m6-split.mjs"]
		      :refactor-rule "KB remains opt-in until memory is cleaned; operational skill facts are not noisy KB and must be explicitly retrievable through the configured MemoryProvider for remote-host, deploy-agent, router embedding/rerank, CI runner, and model-host questions. Project constants should still move to SSOT/Universe rather than worker prompt preloads; broad SSOT review must exclude cold runtime artifacts unless include_runtime=true is explicit.")
    (domain execution-control-plane
      :owner workflow-runner
      :source [autopilot-runtime workstation-config conversation-memory-distillation semantic-ir-shared-memory-convergence evidence-governance-policy file-artifacts mission_request mission_board]
      :functions [work-order-lifecycle workflow-runner task-result-artifact worker-completion-settle slot-lifecycle-manager memory-review-batch-runner board-cleanup-batch-runner board-search-noise-governance evidence-governance-view execution-step-digest conversation-label-calibration taxonomy-proposal jarvis-stream-affinity jarvis-usage-ledger jarvis-chain-monitor]
      :runtime-projection [BoardTask EventBus work_order_intent work_order_plan work_order_audit task_result_artifacts evidence_governance_view conversation ended_at SlotReleased workflow_runs workflow_run memory_review_batch_runner board_cleanup_batch_runner board_search_scope execution_step_digest message_labels token_usage_ledger jarvis_chain_monitor]
      :checker ["node scripts/check-v3-control-plane-m6-split.mjs" "node scripts/check-v3-pty-recognition-isomorphism.mjs" "node scripts/check-v3-workflow-isomorphism.mjs"]
      :refactor-rule "All operator, Board, and external-application delegation requests converge into one work-order lifecycle: intent.lisp or an equivalent external intent envelope is normalized, bound to a BoardTask, compiled into plan.lisp accepted shards, executed through workflow_run/shared-memory, and closed through task-result artifacts plus audit.lisp. Long-running batch work must use checkpointed workflow_run state, canonical task-result artifacts, EventBus-driven completion settle, and slot lifecycle release; Board notes and PTY finals are projections, not the canonical result. Board cleanup is advisory by default: workers write task-result-artifacts and batch reports, generated review tasks may close after settle, and historical BoardTasks remain untouched until an approved maintenance workflow applies recommendations. Board keyword search defaults to active task scope so historical done/skipped tasks do not pollute current governance decisions; historical search requires includeHistorical=true, scope=all, or an explicit historical status. Execution cockpit read models derive execution-step-digest, conversation-label-calibration, taxonomy-proposal, Jarvis stream affinity, Jarvis usage-ledger, and Jarvis chain-monitor summaries from durable tables instead of asking operators to inspect raw PTY.")
	    :workflow ".missiond/workflows/missiond-control-plane-m6-split.lisp"
	    :egress [control-plane-split-report checker-pins compiled-runtime-projection-gaps]
	    :checker "node scripts/check-v3-control-plane-m6-split.mjs")

  (resident-master-control
    :desc "Resident Codex brain layer: event-driven orchestrator that reads the active objective, Lisp SSOT, project registry, explicit context packs, and allowed evidence, then delegates exact work to pool workers."
    :worker codex-master-control
    :slot-id "slot-codex-master-control"
    :engine codex
    :model-profile codex-master-gpt-5-5-xhigh
    :model "gpt-5.5"
    :reasoning-effort xhigh
    :role [orchestrator brain]
    :permissions [read-code read-lisp read-board write-board write-kb write-execution-log dispatch-workers read-event-bus]
    :default-code-write false
    :surfaces [master-checkpoint master-event-subscriber master-decision-loop master-delegation master-recovery night-scheduler commit-lisp-convergence-loop nightly-evolution-loop]
    :checkpoint
      (:sources [mission_execution companion-log BoardTask-note master-control-checkpoint]
       :write-on [turn-start delegation-created delegation-complete daemon-restart-before-exit periodic-heartbeat]
	       :resume-from [latest-master-control-checkpoint open-master-control-boardtask latest-execution-log]
	       :fields [active_objective_id phase context_pack_path delegated_task_ids blocked_reason last_verified_commit resume_instruction]
	       :path-rule "context_pack_path MUST be materialized from the resident master slot's configured MissionD project root, not process cwd, because launchd may start the daemon with cwd=/ and a literal /.missiond path is invalid.")
    :event-subscriptions
      [BoardTaskCreated BoardTaskStatusChanged SlotEvent QuestionEvent SystemEvent::ContextualCommitDetected DaemonRestart StaleTask NightSchedule ProjectRegistryChanged]
    :loop
      ((step s1 :logic "load latest checkpoint, active Board objective, explicit context_pack_path, V3/project Lisp registries, and current event summary")
       (step s2 :logic "perform unknowns-first-intake: ask what information is still missing for this user request, map each unknown to its authority source, and collect bounded evidence before judging intent")
       (step s3 :logic "perform intent-inference: infer the user's immediate objective, durable preference, and governance principle from the active request plus collected evidence")
       (step s4 :logic "perform intent-memory-capture: write or propose a bounded intent memory candidate with evidence_refs, confidence, supersession_scope, and active/needs-review state for future MissionD consciousness evolution")
       (step s5 :logic "ask the heuristic review question and classify whether the objective needs evidence-plan, investigation, design-proposal, exact-shards, implementation, verification, blocked, or no-op")
       (step s6 :logic "for non-exact work, materialize context-pack questions, hypotheses, unknowns, inferred_user_intent, and evidence_needed before delegation; skip directly to implementation only when exact-shard-ready=true is explicit")
       (step s7 :logic "delegate Claude/Gemini/Codex workers through BoardTask/Autopilot only; never bypass durable event/Board state")
       (step s8 :logic "return decision, reasoning_summary, unknowns, inferred_user_intent, intent_memory_candidate, evidence_needed, delegation_plan?, and next_question_or_action, then write checkpoint + Board note + execution companion log after every decision boundary"))
	    :evidence-authority
	      ((tier t1 :source [provider-jsonl codex-local-index claude-jsonl gemini-chat-file] :use "durable final/progress facts")
	       (tier t2 :source [missiond-event-bus BoardTask-lifecycle mission_execution] :use "causal workflow state")
	       (tier t3 :source [provider-aware-pty-recognition screen-buffer] :use "diagnostic state only; never sole completion authority"))
	    :pty-retention
	      (:ttl-days 1
	       :scope [screen-buffer screenshots pty-log-files slot-last-responses]
	       :rule "PTY content is transient diagnostic evidence only. MissionD keeps provider JSONL/Codex provider-local index/Gemini chat files as durable logs, but PTY screen buffers, screenshots, pty-*.log files, and slot_last_responses MUST be treated as short-lived cache with a one-day retention window. Retention cleanup MUST be able to write a delete manifest so applied file/database removal remains reviewable and reversible by evidence.")
	    :settle-policy
	      "A worker can be closed only through canonical task_result_artifact acceptance plus typed worker_settle artifact_hash; durable provider finals, high-confidence summaries, settle windows, and idle PTY state are observation/candidate evidence only."
    (master-checkpoint
      :entry [daemon-startup event-wakeup periodic-heartbeat daemon-restart-before-exit]
      :core
        ((step s1 :logic "read latest event cursor, queue counters, blocked reason, MCP readiness, and current objective")
         (step s2 :logic "record daemon restart/startup context for checkpoint visibility without calling notify or incrementing queued control events")
         (step s3 :logic "resolve checkpoint root from the V3-projected master slot project_root/cwd; never infer it from daemon process current_dir")
         (step s4 :logic "render master-control-checkpoint.lisp under MISSIOND_RUNTIME_DIR when deployed, with repo .missiond/v3/runtime kept only as dev fallback; last-control-prompt is nil for heartbeat/startup ticks and present only when a control turn is dispatchable")
	         (step s5 :logic "store active_objective_id, phase, context_pack_path, delegated_task_ids, blocked_reason, last_verified_commit, and resume_instruction"))
	      :egress ["$MISSIOND_RUNTIME_DIR/master-control-checkpoint.lisp" ".missiond/v3/runtime/master-control-checkpoint.lisp" "mission_master_status.checkpoint" "mission_convergence_status.runtime_status.checkpoint"]
      :surfaces ["crates/missiond-daemon/src/engine/master_control.rs::write_startup_checkpoint_for_slot"
                 "crates/missiond-daemon/src/engine/master_control.rs::render_checkpoint"])
    (master-event-subscriber
      :entry [BoardEvent SlotEvent QuestionEvent IncidentEvent ProjectRegistryChanged DaemonRestart StaleTask NightSchedule]
      :core
        ((step s1 :logic "subscribe to BoardEvent, SlotEvent, QuestionEvent, and IncidentEvent with live-only v2 subscription names, StartFrom::Latest, and PerEvent cursor flush so daemon restart does not replay historical backlog")
         (step s2 :logic "ignore slot-codex-master-control self SlotEvents so the resident brain cannot trigger an infinite self-prompt loop")
         (step s3 :logic "ignore seq=0 volatile events, ordinary SlotEvent.became_idle noise, and SlotEvent.task_dispatched worker lifecycle noise; PTY idle/task-dispatched is diagnostic evidence, not a master-control wakeup authority")
         (step s4 :logic "filter worker creation/running noise before the model while preserving terminal worker edges: swarm worker TaskCreated and dev/running updates do not wake the resident master, but status_changed/updated done/completed/closed/failed/blocked MUST wake it so parent objectives can advance from durable worker completion")
         (step s5 :logic "filter swarm-created worker BoardTasks such as Investigate context for swarm objective, Survey exact shards for swarm objective, and Implement accepted swarm shard, because those terminal worker units belong to Autopilot/provider evidence rather than recursive master delegation")
         (step s6 :logic "same-process Board tool handlers also call notify_board_event_direct immediately after durable DB mutation and before/alongside event-log publish, so master wakeup is not blocked behind dispatcher backlog; Board notes authored by codex-master-control or the legacy resident-codex-master/resident-master aliases MUST NOT direct-notify the master again")
         (step s6b :logic "IncidentEvent live subscription wakes the master only on Lisp-pinned MCP-recovery kinds claude_code_mcp_missing and claude_code_mcp_reconnect_failed (matched against MissionIncident.raw_payload.kind stamped by pty_event_worker::handle_mcp_tool_error); other incident kinds flow through aiops/question-incident pipelines and are diagnostic-only here so MCP recovery does not depend on the operator noticing PTY screen output")
         (step s7 :logic "record only wakeup metadata and ack immediately")
         (step s8 :logic "notify master-decision-loop; never run long worker dispatch inline"))
      :egress [master-control-runtime.event-cursor master-control-runtime.queued-events master-control-runtime.notify]
      :surfaces ["crates/missiond-daemon/src/engine/master_control.rs::spawn_master_event_subscriber"
                 "crates/missiond-daemon/src/engine/master_control.rs::spawn_incident_event_sub"
                 "crates/missiond-daemon/src/engine/master_control.rs::should_wake_for_incident_event"])
    (master-decision-loop
      :entry [master-control-runtime.notify periodic-heartbeat]
      :core
        ((step s1 :logic "probe codex MCP server readiness from codex mcp list and unattended approval readiness from ~/.codex/config.toml tool approval_mode entries")
         (step s2 :logic "classify phase as observe_event -> classify_objective -> create_context_pack -> dispatch_investigators -> compile_shards -> dispatch_implementers -> verify -> close_or_backfill, then materialize context_pack_path as master-control-context-pack.v1 before prompting so the agent does not need to remember file creation")
         (step s2b :logic "no active_objective_id means no control turn and no default self-review; MissionD V3 SSOT self-review is manual-first and runs only through mission_nightly_evolution or MISSIOND_NIGHTLY_EVOLUTION_SCHEDULE=true")
         (step s2c :logic "active_objective_id is the only load-bearing objective: first query exactly that BoardTask by id, follow its description as the load-bearing objective, and read only the project roots/files explicitly named by that BoardTask or its context_pack_path; do not browse Board open backlog")
         (step s2c2 :logic "require MissionD MCP narrowly: active objectives may use only the MCP surfaces needed for that BoardTask. do not call mission_kb_query, mission_conversation_query, provider-log tools, mission_intent, mission_convergence_status, or mission_daemon_update unless the active task explicitly opts them in. A decision of create/update BoardTask or close_or_backfill MUST execute the matching MissionD MCP mutation such as mission_board_note_add, mission_board_update, or mission_board_create before the resident slot returns its final decision; if mutation is unavailable, return blocked")
         (step s2d :logic "mission_convergence_status is a heavyweight diagnostic surface: successful live static snapshots are cached under .missiond/v3/runtime/convergence-status-cache.json, activeRelease reads the blue-green release-manifest typed_lisp_runtime projection hashes, and live timeout returns cached_after_timeout with a warning instead of converting a recent cached OK snapshot into a false blocking item")
         (step s3 :logic "write checkpoint before any durable Board/KB/dispatch action")
         (step s4 :logic "on daemon-startup, ensure slot-codex-master-control is spawned when Exited/Error but do not consume a control turn; startup is for residency, not decision work")
         (step s5 :logic "before sending an event control turn, ensure slot-codex-master-control is spawned when Exited/Error, wait up to 180s for Idle/SlashMenu because gpt-5.5 xhigh control turns are brain-lane work rather than narrow-patch work, and verify the visible Codex footer still matches gpt-5.5 xhigh; if the slot was downgraded by an interactive model/rate-limit prompt, restart it before dispatch")
         (step s6 :logic "send control turns to slot-codex-master-control only when event-wakeup or periodic-heartbeat has a non-terminal active_objective_id, with MCP server ready, required MCP tool approvals ready, and rate-limit guard; SlotEvent noise and queued events without an active objective are checkpoint-only")
         (step s6b :logic "when mission_control pauses the strategy domain or orchestrator slot_role, master-control MUST only write a paused checkpoint and MUST NOT auto-start the resident slot, send a control turn, run nightly evolution, or create self-evolution tasks; this lets operators supervise long-running repair waves without heartbeat restarts breaking provider sessions")
         (step s7 :logic "if a queued objective cannot receive a control turn because Codex master is still starting or the Codex MCP probe is transiently not ready, keep the queued event and retry from periodic-heartbeat once MCP and the slot are ready; successful control turn drains the queued event batch")
         (step s7b :logic "while an active objective exists, periodic-heartbeat MAY run a lightweight objective-followup control turn no more often than every 900s; without an active objective, periodic heartbeat and SlotEvent noise are checkpoint-only and must not trigger self-review")
         (step s7c :logic "classification MUST preserve the current active_objective_id across worker SlotEvents or child BoardTask status events; an event with no top-level task_id must never clear the parent objective")
         (step s7d :logic "when the active parent objective itself emits a terminal BoardEvent.status_changed edge such as Running->Done/Running->done/Failed/Blocked/Skipped or the runtime synthetic Done->terminal closeout edge, detect it case-insensitively, clear active_objective_id, consume the queued event without sending a Codex control turn, and stop periodic heartbeat from reprocessing a completed objective; terminal Board status events must also never create a new active objective during daemon-startup recovery")
         (step s7e :logic "before every tick classification, reconcile the active_objective_id against the durable Board store; if the referenced BoardTask is already Done/Failed/Blocked/Skipped, persistently clear active_objective_id/context_pack_path in the master runtime and write an observe_event checkpoint even if the terminal event was missed or checkpoint state is stale")
         (step s7f :logic "when active_objective_id is present, embed the durable active BoardTask title/status/project/description excerpt into the Codex master control turn; the active BoardTask is the only load-bearing objective, and any mission_board_create call must set parentId to that active objective and directly advance it rather than spawning unrelated resident-master self-maintenance from PTY or convergence-status drift")
         (step s7g :logic "only real top-level BoardEvent.task_created events may create a new active_objective_id; BoardEvent.note_added, test/smoke BoardTask categories, and missing/deleted BoardTasks are diagnostic/checkpoint context only and MUST NOT trigger a resident master control turn")
         (step s8 :logic "detect code-first diffs and create a deduped backfill BoardTask instead of silently accepting Lisp/code drift")
         (step s9 :logic "defer long work to BoardTask/Autopilot; resident master heartbeat must not inspect provider durable logs, KB, Board backlog, historical conversation, or recent commits unless an explicit active BoardTask or dedicated follow-up workflow opts in"))
	      :egress [master-control-checkpoint mission_master_status.service mission_convergence_status]
      :surfaces ["crates/missiond-daemon/src/engine/master_control.rs::spawn_master_decision_loop"
                 "crates/missiond-daemon/src/engine/master_control.rs::build_master_tick_prompt"])
    (master-delegation
      :entry [dispatchable-objective exact-shard context-pack]
      :core
        ((step s1 :logic "create read-only context organizer BoardTasks before code shards")
         (step s2 :logic "compile accepted context-pack into exact file/region write scopes")
	         (step s3 :logic "use mission_swarm_run for productized investigate -> integrate -> implement -> verify fanout, or mission_task_delegate for one exact shard; swarm-created worker BoardTasks are terminal worker units and must not be recursively delegated again")
         (step s4 :logic "require BoardTask ID, context-pack path, read_scope, write_scope, must_not_touch, acceptance, model_profile, timeout_secs in every prompt; read_scope is for reads, write_scope is the only write set, and must_not_touch is a write/stage/commit prohibition rather than a read ban")
         (step s5 :logic "evaluation/review/context-pack worker output is a structured artifact (Findings, Evidence, Recommendations, Verification), not pasted raw KB JSON or full logs"))
      :egress [BoardTaskCreated SlotEvent::TaskDispatched mission_execution])
    (master-recovery
      :entry [daemon-restart startup-checkpoint stale-task durable-provider-log]
      :core
        ((step s1 :logic "load checkpoint event cursor, delegated task ids, blocked reason, and resume plan")
         (step s2 :logic "on daemon-startup, if no queued event survived, recover the latest open/running MissionD BoardTask whose title starts with project SSOT convergence or M6 SSOT convergence, or whose description references project SSOT convergence workflow, and rehydrate it as the active objective")
         (step s3 :logic "reconcile Board open tasks against provider JSONL/Codex provider-local index/Gemini chat files")
         (step s4 :logic "resume or requeue only from durable evidence; PTY recognition is diagnostic")
         (step s5 :logic "never auto-hide, skip, delete, or mutate historical Board cleanup candidates; legacy ops cleanup remains user-directed"))
      :egress [mission_master_status.service BoardTask-note])
    (night-scheduler
      :entry [schedule-metadata manual-objective mission_nightly_evolution]
      :policy (:workflow ".missiond/workflows/nightly-evolution.lisp"
               :schedule-window "manual-first"
               :schedule-enabled false
               :enable-env MISSIOND_NIGHTLY_EVOLUTION_SCHEDULE
               :default-mode observe-only
               :budget-secs 7200
               :max-followup-tasks 3
               :proposal-artifact ".missiond/v3/runtime/self-evolution/<timestamp>-<finding_id>.proposal.lisp"
               :analyzer "scripts/analyze-v3-self-evolution.mjs --json"
               :risk-gate "apply=true creates at most one visible proposal review BoardTask with auto_execute=false; safe-backfill requires low risk, proposal/user-decision modes create review tasks only, and no exact shards or implementation workers are created.")
      :core
        ((step s0 :logic "scheduled nightly evolution is disabled by default during active supervision; operators use manual mission_nightly_evolution or explicitly set MISSIOND_NIGHTLY_EVOLUTION_SCHEDULE=true before periodic runs")
         (step s1 :logic "NightlyEvolutionService reads only ssot-retrieval-scope active-authoring MissionD V3 paths, compiled-semantic-ir, compiled-workflows, V3 checker output, and final convergence static snapshot; Default nightly mode does not read KB, historical conversations, provider logs, worker telemetry, Board open tasks, or recent commit history")
         (step s2 :logic "run scripts/analyze-v3-self-evolution.mjs --json; if compiled runtime is missing, first run node scripts/compile-v3-runtime.mjs --json")
         (step s3 :logic "write observe-first .missiond/v3/runtime/nightly-evolution/<date>.report.lisp plus at most three .missiond/v3/runtime/self-evolution/<timestamp>-<finding_id>.proposal.lisp artifacts sorted low risk first then finding id")
         (step s4 :logic "materialize exactly one visible proposal review BoardTask only when apply=true, requested mode matches finding class, and risk gate allows it; the task must carry auto_execute=false and must not create exact shards or worker implementation tasks")
         (step s5 :logic "checkpoint before and after each batch so daemon restart can resume"))
      :egress [nightly-evolution-report self-evolution-proposal BoardTaskCreated master-control-checkpoint mission_master_status.nightlyEvolution])
    (commit-lisp-convergence-loop
      :entry [SystemEvent::ContextualCommitDetected mission_execution.complete provider-durable-log]
      :core
        ((step s1 :logic "CommitConvergenceService subscribes to SystemEvent::ContextualCommitDetected with StartFrom::Latest and PerEvent cursor flush")
         (step s2 :logic "resolve project from slot project_root/cwd, provider conversation project/project_id metadata, or project registry; commits unavailable in registered local repos are classified external-or-unavailable-commit and write diagnostic reports only")
         (step s3 :logic "read committed snapshot with git diff-tree --root --no-commit-id -r --name-only <sha>, never current worktree diff")
         (step s4 :logic "classify changed files into code, lisp, checker, evidence, docs, or other")
         (step s5 :logic "for code-only commits create one visible deduped BoardTask commit-lisp-backfill:<project>:<sha>; lisp/checker/evidence-only commits do not recurse")
         (step s6 :logic "write .missiond/v3/runtime/commit-lisp-convergence/<sha>.report.lisp and expose commitConvergence status"))
      :egress [commit-convergence-report backfill-boardtask mission_master_status.commitConvergence])
    (lisp-code-sync-loop
      :entry [SystemEvent::ConfigChanged project-registry file-watcher]
      :policy (:workflow ".missiond/workflows/lisp-code-sync.lisp"
               :watch-env MISSIOND_LISP_CODE_SYNC_WATCH
               :default-watch-enabled true
               :queue-table lisp_code_sync_jobs
               :dedupe-key "lisp-code-sync:<project>:<path-hash>"
               :rule "Lisp/checker edits under .missiond are observed through EventBus, subscription only enqueues durable lisp_code_sync_jobs, reconciler claims due jobs with lease before compile/check, runtime report paths are ignored, unchanged content fingerprints are suppressed, debounce repeated path events, retention/GC bounds report volume, and only failing gates create visible BoardTasks.")
      :core
        ((step s1 :logic "watch active ProjectRegistry .missiond directories recursively for .lisp and .mjs changes")
         (step s2 :logic "publish each relevant filesystem change as SystemEvent::ConfigChanged; sync subscriber rechecks path/content fingerprint, upserts lisp_code_sync_jobs, and does not compile/check inline")
         (step s3 :logic "resolve project by longest-prefix ProjectRegistry match")
         (step s4 :logic "reconciler claims due jobs with lease, batches project work, and for missiond runs compile-v3-runtime then check-v3-code-isomorphism-complete; for external projects run .missiond/check.sh when present")
         (step s5 :logic "write .missiond/v3/runtime/lisp-code-sync/<timestamp>-<path-hash>.report.lisp with synced/needs-sync/observed-only status")
         (step s6 :logic "ignore .missiond/v3/runtime/** and .missiond/runtime-state/** before EventBus publication so self-generated reports cannot recurse")
         (step s7 :logic "suppress unchanged content fingerprints at both watcher publication and subscription consumption, debounce repeated path events, and apply report retention/GC to keep the sync loop bounded")
         (step s8 :logic "on failed code-isomorphism create or reuse one visible BoardTask lisp-code-sync:<project>:<path-hash> that requires evidence-plan and exact accepted shard before code mutation")
         (step s9 :logic "Autopilot revalidates lisp-code-sync runtime report BoardTasks before slot selection; tasks that point at .missiond/v3/runtime/lisp-code-sync/** are closed as resolved_by_runtime_fix/stale_evidence and never sent to PTY")
         (step s10 :logic "expose lispCodeSync status and queue metrics queued/running/due/failed/oldest_due_age/active_leases/batch_last_result in mission_master_status so the resident master and frontend can see the live Lisp->code loop"))
      :egress [lisp-code-sync-report sync-boardtask mission_master_status.lispCodeSync])
    (nightly-evolution-loop
      :entry [night-scheduler mission_nightly_evolution final-convergence-snapshot]
      :core
        ((step s1 :logic "collect evidence only from MissionD V3 active-authoring Lisp, compiled-semantic-ir, compiled-workflows, V3 checker output, and final convergence static snapshot")
         (step s2 :logic "run scripts/analyze-v3-self-evolution.mjs --json to detect final-convergence-blocker, facade-budget-near-limit, oversized-authoring-block, and surface-flow-gap")
         (step s3 :logic "classify findings as safe-backfill, needs-investigation, architecture-proposal, or requires-user-decision; analyzer errors become self-evolution-analyzer-error findings with diagnostics")
         (step s4 :logic "default observe-only writes report/proposal artifacts; write at most three proposal artifacts with :proposal_id :finding_id :class :risk :summary :evidence_refs :affected_surfaces :recommended_change :acceptance :non_goals :created_at")
         (step s5 :logic "apply=true creates exactly one visible proposal review BoardTask with auto_execute=false and never creates exact shards or implementation workers"))
      :egress [nightly-evolution-report self-evolution-proposal proposal-review-boardtask master-control-checkpoint])
    :mcp-readiness
      (:source "~/.codex/config.toml"
       :probe "codex mcp list"
       :required-server missiond
	       :required-tool-approvals [mission_intent mission_board_query mission_conversation_query mission_kb_query mission_board_create mission_board_update mission_board_note_add mission_kb_remember mission_task_delegate mission_compute_slot mission_master_status mission_convergence_status mission_nightly_evolution mission_slots mission_pty_status]
	       :status-surface [mission_master_status.mcpReady mission_master_status.mcpEnabled mission_master_status.mcpApprovalReady mission_convergence_status.ok])
    :checker "node scripts/check-v3-master-control-isomorphism.mjs")

  (lisp-code-drift-policy
    :desc "Governance rule for code changes that arrive before their Lisp SSOT design delta."
    :normal-rule "Feature/code changes must map to a V3/project blueprint surface changed in the same task or to an existing surface whose checker pins the behavior; BoardTask close-to-done is blocked while unbackfilled code-first drift exists."
    :emergency-waiver
      (:allowed true
       :requires [waiver-id reason changed-files affected-surface expiry]
       :egress "Create a backfill BoardTask that adds Lisp, checker, and evidence immediately after the emergency fix.")
    :backfill-task
      (:title "Backfill Lisp/checker for emergency code change"
       :owner resident-master-control
       :trigger "git diff contains code surfaces under crates/packages/scripts but no .lisp or evidence delta"
       :dedupe-key "lisp-code-drift:<changed-code-file-hash>"
       :must-include [blueprint-delta checker-delta evidence-note surface-map])
    :close-gate
      (:entry [mission_board_update mission_board_batch_update mission_board_toggle]
       :rule "status=done/toggle-to-done calls drift guard before mutating the task; on drift, return a structured error and keep the original task open while creating/reusing the visible backfill task")
    :checker "node scripts/check-v3-direct-code-drift-policy.mjs")

  (hot-reload-policy
    :desc "Safe self-improvement boundary: Lisp/prompt/pool/signature/frontend config may reload at runtime; Rust code still rolls through deploy/restart."
    :runtime-reload [v3-lisp project-blueprints workstation-pool pty-signatures prompt-contracts frontend-config]
    :rolling-restart [rust-daemon mcp-tools pty-runtime]
    :forbidden [unsafe-dylib-hot-swap hidden-git-state-mutation restart-without-checkpoint]
    :rule "Before daemon restart, resident master-control writes a checkpoint; after restart, Autopilot/master resumes from Board + execution log instead of PTY-only memory.")

  (event-causality-runtime
    :desc "MissionD nervous-system contract: every meaningful runtime transition emits an event with causal ancestry, and downstream muscles subscribe to events instead of polling hidden state."
    :event-chain
      ((user-message
         :event MessageEvent::Logged
         :causes [mission-request-created intent-alignment-drafted])
       (mission-request-created
         :event RequestEvent::Created
         :caused-by MessageEvent::Logged
         :causes [intent-alignment-drafted])
       (intent-alignment-drafted
         :event ArtifactEvent::Written
         :caused-by mission-request-created
         :causes [plan-drafted])
       (plan-drafted
         :event ArtifactEvent::Written
         :caused-by intent-alignment-approved
         :causes [task-runner-manifest-materialized])
       (task-runner-manifest-materialized
         :event TaskRunnerEvent::ManifestMaterialized
         :caused-by plan-approved
         :causes [BoardEvent::TaskCreated])
       (board-task-created
         :event BoardEvent::TaskCreated
         :caused-by task-runner-manifest-materialized
         :triggers [autopilot-runtime.dispatch-boardtask resident-master-control.tick])
       (slot-became-idle
         :event SlotEvent::BecameIdle
         :triggers [autopilot-runtime.dispatch-boardtask resident-master-control.tick])
       (boardtask-prompt-sent
         :event SlotEvent::TaskDispatched
         :caused-by BoardEvent::TaskCreated
         :causes [ExecutionEvent::Completed BoardEvent::StatusChanged]))
    :autopilot-trigger-contract
      "BoardEvent::TaskCreated, BoardEvent::Updated(status=open), BoardEvent::StatusChanged(new_status=open), and SlotEvent::BecameIdle MUST wake Autopilot through event-bus subscribers. Subscribers only notify the dedicated Autopilot task and ack immediately; they MUST NOT run pty.send inline."
    :waiting-contract
      "BoardTask/Slot/Conversation/Execution waits MUST use EventBus-driven waiters whenever a causally linked event exists. Fixed sleep/poll loops are permitted only as bounded fallback with an explicit diagnostic reason; PTY idle is a diagnosis, not task-completion authority."
    :conversation-final-contract
      "Provider durable finals MUST produce a conversation-final/settled transition before Autopilot closes delegated work. Completion tails rebind conversation.task_id, mark worker conversations completed with ended_at, and publish enough event evidence for frontends and master-control to advance without polling hidden state."
    :causation-rule
      "Every generated artifact/event should preserve a predecessor handle when the producer has one. Missing causation is allowed only at external ingress boundaries such as the first user message."
    :implementation ["crates/missiond-daemon/src/bus/v2_subscribers.rs"
                     "crates/missiond-daemon/src/handlers/knowledge/board/events.rs"
                     "crates/missiond-daemon/src/handlers/compute/task_delegate.rs"
                     "crates/missiond-core/src/event/events/board.rs"
                     "crates/missiond-core/src/event/events/slot.rs"]
    :checker "node scripts/check-v3-autopilot-runtime-isomorphism.mjs")

  (autopilot-policy
    :desc "Lisp-owned Autopilot operational windows; tick/reaper/watchdog/consciousness timings are runtime projections, not independent Rust literals."
    :stale-conversation-minutes 10
    :slot-task-reap-stale-secs 1800
    :recover-stale-running-minutes 15
    :slot-failure-throttle-secs 1800
    :deploy-review-timeout-secs 600
    :dynamic-slot-expiring-soon-secs 900
    :stale-board-progress-minutes 30
    :completed-job-gc-minutes 30
    :idle-persistent-slot-secs 1800
    :recent-intents-window-secs 1800
    :user-stuck-cooldown-secs 1800
    :direction-shift-cooldown-secs 3600
    :invariants
      ["AutopilotRuntimeConfig MUST load autopilot-policy from .missiond/v3/missiond-blueprint.lisp and fail with V3_BLUEPRINT_CONFIG_ERROR for real MissionD projects whose V3 blueprint is missing."
       "Autopilot tick windows (stale conversations, stale slot-task reaper, stale running fallback, dynamic-slot expiring-soon warning, completed-job GC, idle persistent slot scale-to-zero, stale board progress reminders) MUST project from autopilot-policy."
       "Autopilot dispatch windows (slot failure throttle and deploy-review memory-slow pty.send timeout) MUST project from autopilot-policy."
       "Autopilot consciousness windows (recent-intents query window, user-stuck cooldown, and direction-shift/scope-creep cooldown) MUST project from autopilot-policy."]
    :checker "node scripts/check-v3-workstation-config-isomorphism.mjs")

  (cascade-policy
    :desc "Lisp-owned universe cascade runtime policy; env vars and caller args are explicit overrides, not hidden defaults."
    :default-manifest "$MISSIOND_PROJECTS_DIR/universe.intent.lisp"
    :allowed-root "$MISSIOND_PROJECTS_DIR"
    :trigger-enabled true
    :default-max-cycles 3
    :max-cycles-limit 12
    :env-overrides [UNIVERSE_MANIFEST UNIVERSE_ROOT CASCADE_TRIGGER_ENABLED]
    :invariants
      ["cascade/path.rs MUST project default manifest and allowed root from cascade-policy before consulting env overrides."
       "mission_cascade_trigger MUST project trigger-enabled and max-cycle bounds from cascade-policy; CASCADE_TRIGGER_ENABLED may only override the V3 switch explicitly."
       "A real MissionD project with .missiond but no V3 cascade-policy MUST return V3_BLUEPRINT_CONFIG_ERROR rather than silently using embedded defaults."])

  (flow-runtime-policy
    :desc "Lisp-owned defaults for mission_flow_run YAML nodes; explicit YAML fields win, missing node fields project from this policy."
    :llm-call-default-max-tokens 65536
    :slot-task-default-model "opus"
    :slot-task-default-timeout-secs 3600
    :parallel-slot-default-parallelism 3
    :parallel-slot-default-timeout-secs 1800
    :invariants
      ["mission_flow_run MUST project missing FlowDefinition node defaults from flow-runtime-policy."
       "Explicit Flow YAML node fields MUST win over flow-runtime-policy defaults."
       "A real MissionD project with .missiond but no flow-runtime-policy MUST return V3_BLUEPRINT_CONFIG_ERROR rather than silently using embedded defaults."])

  (compute-runtime-policy
    :desc "Lisp-owned defaults for low-level compute primitives that spawn or resume PTY-backed slots outside workstation-config."
    (timeout-policy tracked-pty-spawn
      :default_secs 30
      :min_secs 1
      :max_secs 600)
    :invariants
      ["mission_agent spawn/restart and mission_task_submit auto-spawn MUST project tracked PTY spawn wait_for_idle timeout from compute-runtime-policy timeout-policy tracked-pty-spawn."
       "A real MissionD project with .missiond but no compute-runtime-policy MUST return V3_BLUEPRINT_CONFIG_ERROR rather than silently using embedded defaults."])

  (minimax-runtime-policy
    :desc "Lisp-owned defaults for direct MiniMax HTTP gateway calls used by background briefing, translation, and legacy minimax lanes."
    :model "MiniMax-M2.5-highspeed"
    :direct-http-timeout-secs 30
    :quota-throttle-secs 60
    :default-max-tokens 500
    :invariants
      ["MiniMaxClient model, HTTP timeout, and default max_tokens MUST project from minimax-runtime-policy instead of local constants."
       "MinimaxGateway quota throttle sleep MUST project from minimax-runtime-policy instead of a local 60s literal."
       "A real MissionD project with .missiond but no minimax-runtime-policy MUST return V3_BLUEPRINT_CONFIG_ERROR rather than silently using embedded defaults."])

  (router-runtime-policy
    :desc "Lisp-owned defaults for router chat and internal Router/Sonnet gateway calls; direct caller args still win."
    :default-chat-model "gemini-3.1-pro"
    :chat-default-max-tokens 16384
    :file-chat-default-max-tokens 65536
    :flow-gemini-model "gemini-3.1-pro"
    :stateless-sonnet-model "claude-sonnet"
    :queued-sonnet-model "claude-sonnet"
    :anthropic-urgent-model "claude-opus-4-6"
    :anthropic-ops-model "claude-sonnet-4.5"
    :anthropic-docs-test-chore-model "claude-haiku-4-5-20251001"
    :compress-model "gemini-3.1-pro"
    :compress-channel "google"
    :compress-max-tokens 2048
    :compress-char-budget-chars 100000
    :direct-http-timeout-secs 60
    :router-chat-idle-timeout-secs 600
    :router-chat-retry-max-attempts 3
    :router-chat-retry-initial-backoff-ms 250
    :router-chat-retry-max-backoff-ms 2000
    :gemini-pty-queue-timeout-secs 30
    :gemini-http-queue-timeout-secs 300
    :gemini-file-upload-timeout-secs 600
    :gemini-file-poll-timeout-secs 300
    :gemini-cli-absolute-timeout-secs 900
    :gemini-cli-tool-exec-timeout-secs 300
    :queued-sonnet-quota-throttle-secs 30
    :queued-sonnet-default-max-tokens 1024
    :invariants
      ["RouterRuntimeConfig MUST load router-runtime-policy from .missiond/v3/missiond-blueprint.lisp and fail with V3_BLUEPRINT_CONFIG_ERROR for real MissionD projects whose V3 blueprint or policy block is missing."
       "mission_router_chat default model and max_tokens MUST project from router-runtime-policy; model_route_outcomes is a non-core optional feature and may influence scoring only when route learning is explicitly enabled and enough matching task_class samples exist, otherwise router selection falls back to compiled policy; explicit caller model/max_tokens still wins."
       "mission_router_chat default idle_timeout MUST project from router-runtime-policy router-chat-idle-timeout-secs; explicit caller idle_timeout still wins."
       "mission_router_chat transient retry max attempts and bounded exponential backoff MUST project from router-runtime-policy; hard failures remain structured errors, and successful calls that retried MUST include retry diagnostics."
       "mission_router_chat_manage history lookup and compression model/channel/token/char budgets MUST project from router-runtime-policy."
       "OpenAI-compatible chat-completions proxy mode MUST carry the full caller transcript as a direct prompt and MUST NOT send /clear to a shared PTY; clearing an interactive slot is a state mutation and PTY remains diagnostic/shared state."
       "Flow daemon Gemini calls, stateless Sonnet calls, and queued SonnetGateway calls MUST project their model and direct HTTP timeout from router-runtime-policy."
       "GeminiPtyDriver default slot model MUST project from router-runtime-policy flow-gemini-model; explicit caller model still wins."
       "Gemini CLI transport missing llm.yaml model MUST project from router-runtime-policy flow-gemini-model; explicit llm.yaml gemini_cli.model still wins."
       "GeminiClient CLI mode MUST forward non-empty caller model to GeminiCli and use the V3-projected GeminiCli default only when the caller omits model."
       "GeminiClient PTY/HTTP request queue timeouts MUST project from router-runtime-policy, preserving PTY starvation protection without local 30s/300s literals."
       "Gemini File API upload and poll timeouts MUST project from router-runtime-policy instead of local 600s/300s literals."
       "Gemini CLI absolute and tool-exec timeouts MUST project from router-runtime-policy instead of local 900s/300s literals."
       "Queued SonnetGateway quota throttle sleep MUST project from router-runtime-policy instead of a local 30s literal."
       "Thinking-message translation worker is retired: MissionD MUST NOT automatically translate provider internal thinking logs or drain historical thinking backlog through queued Sonnet/router. Any future translation path must be explicit, user-visible, bounded, and attributed."
       "xjp-router embedding client MUST project its missing timeout default from router-runtime-policy direct HTTP timeout; explicit llm.yaml timeout_secs still wins."
       "BoardTask urgent/ops/docs-test-chore ANTHROPIC_MODEL overrides MUST project from router-runtime-policy, not Rust literals."])

  (full-os-feature-gate-policy
    :schema "missiond.full-os-feature-gate-policy.v1"
    :owner control-plane-kernel
    :status code-aligned
    :kernel-core [delegate claim-lease capability spawn attempt completion-artifact settle event-log board-projection pty-worker-adapter]
    :optional-layers [workflow memory skill-store router-experiments codex-replay self-evolution advanced-conversations infra-os advanced-board]
    :feature-gates [MISSIOND_FULL_OS_ENABLE MISSIOND_FEATURE_WORKFLOW_ENABLE MISSIOND_FEATURE_MEMORY_ENABLE MISSIOND_FEATURE_SKILL_STORE_ENABLE MISSIOND_FEATURE_ROUTER_EXPERIMENTS_ENABLE MISSIOND_FEATURE_CODEX_REPLAY_ENABLE MISSIOND_FEATURE_SELF_EVOLUTION_ENABLE MISSIOND_FEATURE_CONVERSATIONS_ENABLE MISSIOND_FEATURE_INFRA_OS_ENABLE MISSIOND_FEATURE_BOARD_ADVANCED_ENABLE]
    :rule
      ["MissionD kernel-core keeps delegate, claim/lease, capability, spawn, attempt, completion artifact, settle, event_log, Board projection, and the minimal PTY worker adapter enabled by default."
       "mission_compute_slot(create) for task-bound dynamic workers MUST bind spawn to a current job_attempt via ControlPlaneKernel::start_attempt_command after PTY spawn succeeds and before the slot is announced idle."
       "workflow, memory/KB, skill-store, router experiments, codex replay, self-evolution/Lisp code sync, advanced conversations, infra OS operations, and advanced Board decomposition are full-os optional layers."
       "Optional MCP tool names remain public but dispatch MUST return structured FEATURE_DISABLED unless MISSIOND_FULL_OS_ENABLE or the matching MISSIOND_FEATURE_* gate is enabled."
       "Optional startup services MUST NOT run in kernel-core mode."
       "Blue-green launchd deployment MUST propagate MISSIOND_FULL_OS_ENABLE and individual MISSIOND_FEATURE_* gates from the caller environment so Mac mini can run full MissionD without rsync or manual plist edits."])

  (mechanic-collaboration-boundary
    :schema "missiond.mechanic-collaboration-boundary.v1"
    :status mechanic-executor-lane-enabled
    :owner missiond
    :executor jarvis-mechanic
    :rule "MissionD owns workflows, Board, event bus, project registry, Universe, worker dispatch, checkpoints, and approval. Jarvis Mechanic is only an opt-in repair executor used after MissionD has produced an explicit repair shard."
    :missiond-responsibilities [ssot-universe workflow-lisp boardtask-lifecycle eventbus resident-master-control swarm-dispatch approval checkpoint verification]
    :mechanic-responsibilities [compiler-as-judge-repair narrow-code-transform dry-run-diagnostics patch-proposal repair-report]
    :runtime-policy (:enabled true
                     :default-mode dry-run
                     :delegate-entry mission_task_delegate
                     :engine-hint mechanic
                     :allowed-entrypoints [approved-repair-shard dry-run-diagnostics]
                     :forbidden-entrypoints [resident-master-control night-scheduler nightly-evolution-loop autonomous-orchestrator generic-worker-pool])
    :handoff-contract (:entry [BoardTaskApproved accepted_shard_id context_pack_path write_scope acceptance mechanic_mode]
                       :core ((step s1 :logic "MissionD discovers/collates the issue through checker/EventBus/audit and resident-master-control; Mechanic never performs broad architecture discovery.")
                              (step s2 :logic "MissionD creates an accepted exact repair shard with project_id, files, allowed ranges, acceptance, rollback policy, and a shared-memory write lease.")
                              (step s3 :logic "mission_task_delegate routes engine_hint=mechanic only when accepted_shard_id, context_pack_path, and write_scope are present; mechanic implementation requests without exact shard metadata are rejected before BoardTask creation; it creates a visible non-autoExecute BoardTask so Autopilot/PTY dispatch cannot reroute mechanic work to ClaudeCode; default mechanic_mode is dry-run, repair requires explicit mechanic_mode=repair.")
                              (step s4 :logic "Mechanic runs compiler-as-judge repair only inside the approved write_scope or returns a no-change diagnostic.")
                              (step s5 :logic "Mechanic stdout/stderr/exit status is normalized into task-result-artifact; Board note/provider final/PTY are projections only.")
                              (step s6 :logic "MissionD verifies diff, checker, tests, commit/Lisp convergence, and releases shared-memory claims before closing the parent BoardTask."))
                       :egress [task-result-artifact patch-proposal repair-report verification-result])
    :safety ["Mechanic MUST NOT read Board/KB/provider logs as an architect unless MissionD passes a bounded context pack."
             "Mechanic MUST NOT be called by nightly-evolution by default."
             "Mechanic MUST NOT become a resident master or project orchestrator."
             "Mechanic runtime enablement is limited to mission_task_delegate engine_hint=mechanic with accepted_shard_id, context_pack_path, write_scope, and acceptance; standalone mission_mechanic tools remain forbidden."]
    :checker "node scripts/check-v3-mechanic-boundary-isomorphism.mjs")
