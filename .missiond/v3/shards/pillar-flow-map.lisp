  (pillar-flow-map
    :schema "missiond.pillar-flow-map.v1"
    :rule "Each pillar owns functions; each function declares entry -> ordered core steps -> egress, and each function maps back to exactly one implementation-map surface."
    :function-shape "function must carry :surface, non-empty :entry, :core ((step s1 :logic ...) ...), and non-empty :egress."

    (pillar request
      (function request-lifecycle
        :surface mission_request
        :entry [mission_request.start mission_request.advance mission_request.status mission_request.respond]
        :core ((step s1 :logic "persist request-local request.lisp and lifecycle event")
               (step s2 :logic "project review_packet from request-local artifacts and latest review event")
               (step s3 :logic "route respond actions through directive/plan gates without bypassing them")
               (step s4 :logic "materialize directive_id / plan_id / board_task_id back into request-local Lisp"))
        :egress [request-local-artifacts review_packet respond_result blocked_response])
      (function unified-entry-runtime
        :surface unified-entry-runtime
        :entry [unified_entry.run_pipeline mission_request.advance]
        :core ((step s1 :logic "normalize staged pipeline request")
               (step s2 :logic "dispatch s1/s3/s4/s5/s6 stages to directive, plan, or execution handlers")
               (step s3 :logic "decorate response with pipeline_stage, flow_ref, artifact_refs, and next_step"))
        :egress [pipeline_response artifact_refs next_step]))

    (pillar artifacts
      (function file-artifact-writer
        :surface file-artifacts
        :entry [attempt_artifact_write atomic_write_artifact artifact_path]
        :core ((step s1 :logic "resolve artifact kind to request-local or compatibility path")
               (step s2 :logic "write through temp-file, fsync, and atomic rename discipline")
               (step s3 :logic "return Written / ResolveFailed / WriteFailed without hiding partial states"))
        :egress [artifact_path write_status write_failed_diagnostic]))

    (pillar intent
      (function directive-authoring
        :surface mission_directive
        :entry [mission_directive.compile mission_directive.approve]
        :core ((step s1 :logic "compile deterministic draft or validate Sonnet Lisp")
               (step s2 :logic "persist directive row and enrich Lisp with directive_id/version")
               (step s3 :logic "emit alignment review gate without auto-approval"))
        :egress [intent-alignment.lisp directive_row review_question_warning]))

    (pillar plan
      (function plan-authoring-and-runner
        :surface mission_plan
        :entry [mission_plan.compile mission_plan.approve mission_plan.execute]
        :core ((step s1 :logic "compile executable plan-draft with target/objective/nodes hints")
               (step s2 :logic "approve persisted plan only through plan-review gate")
               (step s3 :logic "parse plan Lisp hints into DAG/internal dispatch arguments")
               (step s4 :logic "forward execution to mission_execution, mission_task_delegate, or workflow substrate"))
        :egress [plan.lisp plan_row execute_result task_brief]))

    (pillar verification
      (function evidence-collector
        :surface evidence-collector
        :entry [EvidenceEntry.append wrap_legacy_record_evidence EventRefResolver]
        :core ((step s1 :logic "stamp schema_version/source/kind and preserve caller evidence")
               (step s2 :logic "resolve execution event refs as live, passive_cache, event_log_query, or unavailable")
               (step s3 :logic "append evidence sidecar without overwriting existing entries"))
        :egress [verification-receipt evidence-sidecar event_ref_status]))

    (pillar execution
      (function execution-log
        :surface mission_execution-log
        :entry [mission_execution.open mission_execution.list mission_execution.status session_trace_path]
        :core ((step s1 :logic "read/write companion log as durable Lisp source")
               (step s2 :logic "route execution actions through the manager action table")
               (step s3 :logic "publish ExecutionEvent only after durable write succeeds")
               (step s4 :logic "optionally append structured session-trace event as best-effort projection"))
        :egress [companion-log execution_event trace_warning status_summary])
      (function execution-claim-lease
        :surface mission_execution-claim-lease
        :entry [mission_execution.claim mission_execution.heartbeat mission_execution.release]
        :core ((step s1 :logic "allocate claim id and clamp lease window")
               (step s2 :logic "reject overlapping active claims through scopes_overlap_pure")
               (step s3 :logic "extend heartbeat lease or stamp released-at/status released"))
        :egress [claim_record lease_expires_at released_claim conflict_error])
      (function execution-completion-audit
        :surface mission_execution-completion-audit
        :entry [mission_execution.complete mission_execution.audit mission_execution.repair mission_execution.preflight_commit]
        :core ((step s1 :logic "normalize commit and verifier status enums before mutation")
               (step s2 :logic "enforce scoped commit and task contract completion invariants")
               (step s3 :logic "run daemon auto-verifier when report/shared-memory/contract/commit inputs are present")
               (step s4 :logic "preflight git status read-only and report out-of-scope drift"))
        :egress [completion_record audit_findings preflight_summary repair_summary]))

    (pillar workflow
      (function workflow-distillation
        :surface mission_workflow
        :entry [mission_workflow.distill mission_workflow.compile_methodology mission_workflow.run_methodology]
        :core ((step s1 :logic "validate workflow_sexp or methodology Lisp")
               (step s2 :logic "persist/enrich workflow artifact with source_plans, match_rules, steps, status, and body")
               (step s3 :logic "compile methodology into executable YAML without bypassing receipt-only gates"))
        :egress [workflow.lisp workflow_row compiled_yaml run_result])
      (function typed-lisp-compiler
        :surface typed-lisp-compiler
        :entry [missiond-lispc.check-v3 missiond-lispc.check-workstation-config missiond-lispc.check-workflow missiond-lispc.check-workflow-dir missiond-lispc.check-genome missiond-lispc.check-genome-dir missiond-lispc.check-project missiond-lispc.check-project-dir missiond-lispc.check-auth-domain missiond-lispc.check-m6-depth missiond-lispc.check-domain-hardening-deprecated-alias missiond-lispc.emit-json missiond-lispc.emit-v3 missiond-lispc.emit-runtime-config missiond-lispc.emit-semantic-ir missiond-lispc.emit-universe missiond-lispc.emit-workflows missiond-lispc.emit-genomes]
        :core ((step s1 :logic "parse Lisp SSOT files into source-located typed AST nodes")
               (step s2 :logic "validate pillar/function entry-core-egress surfaces, workflow contracts, universe registry, maturity gates, and event/outbox contracts")
               (step s3 :logic "emit stable JSON diagnostics for JS compatibility wrappers and CI gates")
               (step s4 :logic "generate compiled JSON projections only through checker/compiler commands, never by hand; compiled-runtime-config.json, project universe, and workflows emit structured payloads rather than token-presence booleans")
               (step s5 :logic "let Rust and JS runtime config consumers read compiled-runtime-config.json first only when the compiled snapshot is not older than source Lisp, then diagnostic-fallback to Lisp/default behavior")
               (step s6 :logic "validate the full .missiond/workflows directory with typed AST so old methodology workflows and active runtime workflows carry explicit source plans, risk gates, completion criteria, and step contracts")
               (step s7 :logic "validate project-local .missiond blueprint directories with typed AST before M5 maturity can rely on project-local shape evidence")
               (step s8 :logic "use Auth as the first external project M6-depth semantic checker sample before shrinking more project checkers")
               (step s9 :logic "validate generic Auth-grade M6 evidence before claiming production-ready architecture")
               (step s10 :logic "validate workstation-config semantic policies with typed AST so managed-node portability, provider-unavailable diagnostics, model profiles, slot templates, timeouts, and policy shards are structural gates rather than brittle prose pins"))
        :egress [typed_diagnostics compiled_json compiled_runtime_config compiled_runtime_snapshot compiled_project_universe compiled_workflow_contracts compiled_genomes js_wrapper_result])
      (function genome-runtime
        :surface genome-runtime
        :entry [missiond-lispc.check-genome missiond-lispc.check-genome-dir missiond-lispc.emit-genomes missiond-kernel.AtomRegistry missiond-organism-runtime.AutopilotCell AutopilotOrganRuntime]
        :core ((step s1 :logic "parse genome Lisp into typed genome/organ/tissue/molecule projections and emit compiled-genomes JSON")
               (step s2 :logic "validate tissue receptors, allowed atoms/effects, activation, and runtime budgets before daemon hot paths can consume them")
               (step s3 :logic "run Autopilot Organ in shadow mode by comparing Cell effects against legacy board/slot wakeup helpers and write activation/shadow snapshots under MISSIOND_RUNTIME_DIR when deployed")
               (step s4 :logic "enable active mode only after checker, unit, integration, and shadow parity receipts pass")
               (step s5 :logic "quarantine shadow mismatches or active runtime errors by falling back to legacy Autopilot behavior and publishing IncidentEvent::Reported"))
        :egress [compiled-genomes AutopilotOrgan shadow-report activation-snapshot quarantine-incident])
      (function semantic-ir-compiler
        :surface semantic-ir-compiler
        :entry [missiond-lispc.emit-semantic-ir scripts/compile-v3-runtime.mjs]
        :core ((step s1 :logic "compile source-located Lisp functions and implementation surfaces into compact typed facts")
               (step s2 :logic "assign every fact a stable short id, source hash, file, line, and column for precise back-reference")
               (step s3 :logic "generate compiled-semantic-ir.json, compiled-agent-slices.json, and compiled-workflow-contracts.json for agents and runtime readers")
               (step s4 :logic "keep generated IR machine-oriented and compact while preserving diagnostics and source maps for human review"))
        :egress [compiled-semantic-ir compiled-agent-slices compiled-workflow-contracts source-map-diagnostics])
      (function work-order-lifecycle
        :surface work-order-lifecycle
        :entry [user-request external-application-intent.lisp external-service-intent-envelope BoardTaskCreated mission_board_create mission_request workflow-trigger]
        :core ((step s1 :logic "normalize every source into a work-order intent with objective, inferred user intent, constraints, unknowns, evidence_refs, source app, and optional source_board_task_id")
               (step s2 :logic "apply pre-task-board-anchor: every non-trivial mutation, deploy, skill edit, memory batch, or worker dispatch must bind the intent to exactly one BoardTask before execution; Board-sourced work uses the existing task as anchor, while file/API/external-app intent creates or links one visible BoardTask")
               (step s3 :logic "compile plan.lisp as a forecast with accepted shards, worker lanes, read_scope, write_scope, risk gates, completion authority, and retry policy")
               (step s4 :logic "compile the plan into plan-atomization-graph by splitting shards into atom_tasks, marking dependency_edges, execution_order, serial_groups, and parallel_groups")
               (step s5 :logic "start workflow_run and shared-memory cursor, persist plan/atom graph hashes, and write an audit.lisp replay header before dispatch")
               (step s6 :logic "dispatch workers only from accepted atom_task_contracts; external translation, code-refactor, deploy-ops, and memory-review requests use the same BoardTask/Autopilot chain")
               (step s7 :logic "collect task-result-artifacts and worker-detour-telemetry as canonical outputs; Board notes, provider finals, PTY snapshots, and app callbacks are projections or evidence")
               (step s8 :logic "close or backfill only after durable final settle, audit.lisp append, slot release, and Lisp/checker/evidence convergence when code changes occurred")
               (step s9 :logic "promote repeatable operation patterns into workflow.lisp or evidence, not ad hoc prompt text"))
        :egress [work-order-intent.lisp plan.lisp plan-atomization-graph audit.lisp BoardTask workflow_run atom_task_contracts task_result_artifacts worker-capability-telemetry task_contracts event_log workflow_promotion_candidate]))

    (pillar review
      (function review-gate
        :surface review-gate
        :entry [review_gate_policy review_decision review_question_id]
        :core ((step s1 :logic "normalize manual / emit_question / off policy")
               (step s2 :logic "emit created/resolved review events without blocking primary writes")
               (step s3 :logic "surface deterministic warning ids on bus failure"))
        :egress [review_event review_question_warning review_resolution]))

    (pillar task-runner
      (function task-runner-lifecycle
        :surface task-runner-cli
        :entry [task-runner-next-action task-runner-dispatch task-runner-finalize-report check-verification-receipt]
        :core ((step s1 :logic "append one-event lifecycle files through cooperative lock/create-only writes")
               (step s2 :logic "derive wave state and choose next runnable/finalization action")
               (step s3 :logic "project final-report and verification-receipt into request-local artifacts")
               (step s4 :logic "verify commit-snapshot artifacts against task contract write scope"))
        :egress [lifecycle-event final-report verification-receipt dispatch_event]))

    (pillar source-control
      (function source-hygiene
        :surface source-hygiene
        :entry [check-staged-source-hygiene task-scope-guard pre-commit-hook ssot-retrieval-scope]
        :core ((step s1 :logic "inspect staged or supplied files without mutating git")
               (step s2 :logic "reject raw NUL bytes and git diff whitespace errors")
               (step s3 :logic "enforce task contract write-scope and must-not-touch patterns")
               (step s4 :logic "separate active SSOT authoring paths from warm evidence and cold runtime artifacts for broad search/review, and mirror that boundary into repo-local search ignore sidecars"))
        :egress [source_hygiene_result scope_guard_diagnostics hook_doctor_status ssot_retrieval_scope_diagnostics])
      (function external-work-order-gate
        :surface external-work-order-gate
        :entry [missiond-work-order.start missiond-work-order.verify-staged missiond-work-order.verify-commit pre-commit-hook ci-merge-gate code-first-drift-watcher]
        :core ((step s1 :logic "create or locate .missiond/work-orders/<id>/ with intent.lisp, plan.lisp, audit.lisp, accepted_shard_id, read_scope, and write_scope")
               (step s2 :logic "for staged code changes, require MissionD-Work-Order id from env or commit trailer and require intent.lisp/plan.lisp to exist")
               (step s3 :logic "verify changed code files are covered by accepted shard write_scope before local commit")
               (step s4 :logic "for commit/merge/deploy, rerun missiond-work-order verify --commit <sha> so bypassed hooks cannot land uncovered code")
               (step s5 :logic "if code changes appear outside a work-order, create a visible code-first drift backfill BoardTask instead of silently accepting the diff"))
        :egress [work_order_verify_result precommit_rejection ci_gate_result code_first_drift_boardtask])
      (function lisp-code-drift
        :surface lisp-code-drift-policy
        :entry [git-diff task-contract blueprint-registry emergency-waiver board-task-close]
        :core ((step s1 :logic "map changed files to V3/project blueprint surfaces")
               (step s2 :logic "require same-task Lisp/checker delta for normal behavior changes")
               (step s3 :logic "allow emergency code-first fixes only with waiver metadata")
               (step s4 :logic "create backfill BoardTask for Lisp/checker/evidence convergence")
               (step s5 :logic "block BoardTask close-to-done until drift has a Lisp/checker/evidence backfill"))
        :egress [drift_result waiver_record backfill_boardtask close_blocked_error])
      (function commit-lisp-convergence
        :surface commit-lisp-convergence-loop
        :entry [SystemEvent::ContextualCommitDetected mission_execution.complete provider-durable-log]
        :core ((step s1 :logic "resolve project from commit event slot project_root/cwd or registry longest-prefix match")
               (step s2 :logic "classify commit hashes absent from all registered local project roots as external-or-unavailable-commit so provider-log mentions of external commits do not masquerade as project-registry defects")
               (step s3 :logic "inspect committed snapshot with git diff-tree --root --no-commit-id -r --name-only <sha>")
               (step s4 :logic "classify files into code/lisp/checker/evidence/docs/other")
               (step s5 :logic "covered when code changes have same-commit Lisp/checker/evidence coverage; lisp-only commits do not recurse")
               (step s6 :logic "create one visible deduped backfill BoardTask for code-only commits"))
        :egress [commit_convergence_report backfill_boardtask mission_master_status.commitConvergence])
      (function lisp-code-sync
        :surface lisp-code-sync-loop
        :entry [SystemEvent::ConfigChanged file-watch project-registry]
        :core ((step s1 :logic "watch active project .missiond directories and publish SystemEvent::ConfigChanged for .lisp/.mjs changes")
               (step s2 :logic "subscription consumer only upserts durable lisp_code_sync_jobs; reconciler claims due jobs with lease before compile/check")
               (step s3 :logic "compile typed runtime projection and run the project code-isomorphism checker from reconciler batches")
               (step s4 :logic "treat .missiond/v3/runtime/**, runtime-state, compiled projection, reports, checkpoints, and cold runtime evidence as non-authoring paths; ignore them before EventBus publication and before sync-task creation")
               (step s5 :logic "write a lisp-code-sync report for every accepted authoring event and bound report volume with retention/GC plus reportDir live counters")
               (step s6 :logic "create exactly one visible deduped sync BoardTask for failing gates; use root_cause_key lisp-code-sync:<project>:storm-circuit when same-source failures exceed the storm threshold")
               (step s7 :logic "require master evidence-plan and exact accepted shard before downstream code worker mutation")
               (step s8 :logic "Autopilot closes stale runtime-report sync BoardTasks as resolved_by_runtime_fix/stale_evidence before slot selection so historical self-loop tasks cannot consume worker slots")
               (step s9 :logic "expose stormCircuitHits/recentSyncTaskCreations/reportDirs and queue metrics in mission_master_status so stale decisions can be revalidated without shell archaeology"))
        :egress [lisp_code_sync_report sync_boardtask mission_master_status.lispCodeSync])
      (function same-source-storm-circuit-breaker
        :surface lisp-code-sync-storm-circuit
        :entry [failed-sync-check recent-sync-task-creations dedupe-key]
        :core ((step s1 :logic "count sync BoardTasks created within the storm window")
               (step s2 :logic "when the pre-create threshold is reached, switch from path-hash dedupe to lisp-code-sync:<project>:storm-circuit root-cause dedupe")
               (step s3 :logic "reuse the existing storm circuit task while it is open and append diagnostics through reports/status rather than creating more worker tasks")
               (step s4 :logic "surface storm hits, last circuit timestamp, and recent creation count in mission_master_status"))
        :egress [storm_circuit_task dedupe_hit storm_diagnostic mission_master_status.lispCodeSync]))

    (pillar coordination
      (function context-pack
        :surface context-pack
        :entry [context-pack-append context-pack-compile-shards context-pack-materialize-wave context-pack-run-wave]
        :core ((step s1 :logic "append claim/observation/anchor/shard-proposal/conflict entries with locked seq allocation")
               (step s2 :logic "validate accepted shard references and non-overlap")
               (step s3 :logic "compile integration-plan dispatch groups for code workers")
               (step s4 :logic "materialize mapped dispatch groups into task-runner manifest and task contracts")
               (step s5 :logic "prepare briefs and produce dispatch descriptor or explicit apply submission through the single runner"))
        :egress [context-pack.lisp dispatchable_groups accepted_shards task-runner-manifest task-contracts dispatch_descriptor])
      (function mission-board
        :surface mission_board
        :entry [mission_board.create mission_board.claim mission_board.update mission_board.note_add IncidentEvent::Reported create_pty_remediation_task mission_incident.remediate]
        :core ((step s1 :logic "persist BoardTask status, assignee, dependency, lease, and note state")
               (step s2 :logic "normalize common MCP argument aliases such as task_id/taskId, note_type/noteType, parent_id/parentId, and timeout_secs/timeoutSecs before schema projection")
               (step s3 :logic "return structured ToolError codes for invalid params, missing/not-found tasks, and store failures so agents can recover without flailing")
               (step s4 :logic "claim only open unclaimed rows and recover stale running rows")
               (step s5 :logic "publish BoardEvent projections for dashboards and autopilot; large BoardTask notes return compact receipts after durable storage instead of echoing full content through MCP")
               (step s6 :logic "validate parentId and dependsOn as existing BoardTask IDs or uniquely-resolved short IDs before write; unresolved dependency strings and self-references are structured invalid-param errors, never persisted as latent Autopilot blockers")
               (step s7 :logic "apply Board text-size policy at runtime: create/update descriptions are capped with char-boundary-safe truncation; note_add rejects oversized payloads with a structured artifact-path suggestion instead of unknown MCP errors")
               (step s8 :logic "classify self-heal incidents by semantic dedupe_key, not generated BoardTask id or volatile PTY screen text")
               (step s9 :logic "if an open task with the same dedupe_key exists, append progress evidence and touch it instead of creating another BoardTask")
               (step s10 :logic "runtime-safe recovery may be performed by PTY/slot runtime directly; uncertain MCP/tool failures become visible operator-review incident tasks")
               (step s11 :logic "self-heal incident BoardTasks are not auto-executed by default because automatic worker fanout can amplify a tool outage into a Board storm"))
        :egress [board_task board_event board_note autopilot_task_list incident_boardtask aggregate_note operator_review])
      (function board-search-noise-governance
        :surface board-search-noise-governance
        :entry [mission_board.query.search mission_board_query]
        :core ((step s1 :logic "parse keyword search scope from status, scope, and includeHistorical")
               (step s2 :logic "when no explicit status/history scope is supplied, constrain search to active statuses open/running/verifying/blocked/failed")
               (step s3 :logic "allow historical done/skipped review only through includeHistorical=true, scope=all/historical, or status=done/skipped")
               (step s4 :logic "return meta.activeFilterApplied and meta.historicalIncluded so frontends and agents can tell whether history was excluded")
               (step s5 :logic "steer Board cleanup workflows through explicit historical scope and batch reports instead of broad keyword search defaults"))
        :egress [board_search_result board_search_scope cleanup_candidate_query]))

    (pillar workstation
      (function workstation-config
        :surface workstation-config
        :entry [mission_compute_slot mission_task_delegate autopilot.dispatch_board_tasks]
        :core ((step s1 :logic "derive model/profile, slot env hooks, and suppress_initial_prompt ownership")
               (step s2 :logic "project BoardTask timeout into pty send budget, watchdog, and claim lease")
               (step s3 :logic "send BoardTask prompt through Autopilot-owned pty path with per-slot exclusion"))
        :egress [dynamic_slot board_task_dispatch close_action kb_feedback])
      (function workstation-pool
        :surface workstation-pool
        :entry [workstation-pool mission_compute_slot.list Autopilot.select-workstation-pool-slot]
        :core ((step s1 :logic "read compact V3 pool workers with engine, slot_id, task_class, capability, timeout, and write policy")
               (step s2 :logic "register pool workers into AgentSlotManager and MissionControl runtime slots without persisting legacy slots.yaml state")
               (step s3 :logic "classify unassigned BoardTasks and select an idle pool worker before considering legacy/static slots")
               (step s4 :logic "expose pool status through mission_compute_slot list so Claude/Gemini lanes are observable"))
        :egress [runtime_slot pool_status boardtask_slot_selection])
      (function resident-master-control
        :surface resident-master-control
        :entry [resident-master-control BoardTaskCreated SlotEvent QuestionEvent mission_master_status]
        :core ((step s1 :logic "restore checkpoint from Board/execution log and load recent event tail")
               (step s2 :logic "run unknowns-first-intake: enumerate missing information and authority sources before judging user intent or deciding whether to delegate")
               (step s3 :logic "infer user intent and create an intent_memory_candidate with evidence_refs/confidence/supersession_scope; write high-confidence stable intent to memory:decision through mission_kb_remember, otherwise keep it as a needs-review candidate artifact")
               (step s4 :logic "make top-level decisions and record them before delegation")
               (step s5 :logic "dispatch investigation/context-pack workers before code shards")
               (step s6 :logic "delegate ordinary implementation to Claude/Gemini/Codex pool workers through BoardTask/Autopilot")
               (step s7 :logic "write checkpoint and durable final note after each decision boundary"))
        :egress [master_checkpoint delegated_boardtasks governance_notes mission_master_status])
      (function nightly-evolution
        :surface nightly-evolution-loop
        :entry [night-scheduler mission_nightly_evolution final-convergence-snapshot]
        :core ((step s1 :logic "collect only MissionD V3 active-authoring Lisp, compiled-semantic-ir, compiled-workflows, V3 checker output, and final convergence static snapshot")
               (step s2 :logic "run scripts/analyze-v3-self-evolution.mjs --json and detect final-convergence-blocker, facade-budget-near-limit, oversized-authoring-block, and surface-flow-gap")
               (step s3 :logic "classify findings into safe-backfill, needs-investigation, architecture-proposal, or requires-user-decision; analyzer errors become diagnostic findings")
               (step s4 :logic "write nightly-evolution report plus at most three self-evolution proposal artifacts; apply=true creates one visible proposal review BoardTask with auto_execute=false")
               (step s5 :logic "surface status through mission_master_status.nightlyEvolution"))
        :egress [nightly_evolution_report self_evolution_proposal proposal_review_boardtask mission_master_status.nightlyEvolution])
      (function delegated-boardtask-runtime
        :surface autopilot-runtime
        :entry [BoardEvent.TaskCreated BoardEvent.StatusChanged SlotEvent.BecameIdle board_dispatch_notify autopilot.dispatch_board_tasks]
        :core ((step s1 :logic "subscribe to event-bus BoardEvent and SlotEvent nerves")
               (step s2 :logic "wake the dedicated Autopilot task without running pty.send inside the subscriber")
               (step s3 :logic "claim eligible BoardTask rows, select or provision slots, and hold per-slot dispatch guards")
	               (step s4 :logic "send prompts once through Autopilot, emit SlotEvent::TaskDispatched, wait_for_worker_final_settle_window, and reconcile mission_execution completion")
	               (step s5 :logic "close BoardTask only after durable/high-confidence final evidence settle, close delayed active-frame tasks from durable summary note plus idle slot evidence, or preserve worker self-close/blocked states"))
        :egress [BoardEvent SlotEvent ExecutionEvent board_task_status mission_execution_completion])
      (function workstation-dispatch
        :surface workstation-dispatch
        :entry [mission_plan.execute_internal run_workstation_dispatch_with_contract_and_trace]
        :core ((step s1 :logic "build scoped task brief and dispatch descriptor")
               (step s2 :logic "evaluate dry-run / dispatched / inner-error / safe-descriptor outcome")
               (step s3 :logic "project delegated_board_task_id and task_brief_preview without waiting for worker completion"))
        :egress [WorkstationDispatchOutcome task_brief_preview delegated_board_task_id]))

    (pillar memory
      (function memory-provider
        :surface memory-provider-boundary
        :entry [mission_memory mission_kb_query mission_kb_remember context-pack-builder provider-registry]
        :core ((step s1 :logic "load memory-provider-contract and active provider selection")
               (step s2 :logic "resolve tenant/universe/project/user scope before every memory operation")
               (step s3 :logic "route query/write/review/export operations through provider capability checks")
               (step s4 :logic "preserve local-postgres-memory as compatibility provider while allowing null-memory for open-source/default and xjp-memory for private multi-universe deployments")
               (step s5 :logic "return explicit provider disabled/misconfigured diagnostics rather than silently falling back to stale private data"))
        :egress [MemoryProviderConfig memory-provider-status scoped-memory-result])
      (function codex-boot-context
        :surface codex-boot-context
        :entry [mission_context_boot resident-master-start codex-worker-start external-chat-handoff]
        :core ((step s1 :logic "load the small versioned capsule from .missiond/v3/evidence/codex-boot-context.lisp")
               (step s2 :logic "validate the capsule has L0/L1/L2/L3 layers, required collaboration rules, and no secrets/raw logs/bulk chat history")
               (step s3 :logic "return L0 always-on protocol plus optional L1 task ids; tell the agent to call mission_context_gather for bounded L2 facts")
               (step s4 :logic "keep historical conversations, provider logs, and old Board tasks as L3 cold evidence opened only by explicit audit/debug intent"))
        :egress [codex_boot_context_capsule external_handoff_card boot_context_diagnostic])
      (function knowledge-memory
        :surface memory-kb
        :entry [mission_context_boot mission_context_gather mission_kb_query mission_kb_remember mission_kb_mutate mission_kb_review mission_kb_ops mission_beacon mission_code_search mission_memory mission_insight mission_intent]
        :core ((step s1 :logic "load memory-kb-policy and learning-engine-policy for realtime extraction batch, preview budgets, learning cadences, and pty send budgets")
               (step s2 :logic "resolve project/global memory scope and normalize KB or intent query; high-frequency intent grounding should use mission_context_gather to aggregate KB, SSOT, project registry, skill evidence, infra evidence, active Board task records, and bounded conversation logs before falling back to leaf tools")
               (step s3 :logic "read or mutate durable knowledge rows through one Lisp-described memory contract")
               (step s4 :logic "route all realtime extraction, deep-analysis, manual MCP, and internal learning writes through the shared KB dedupe gate before any new active key can be created")
               (step s5 :logic "merge exact/fuzzy/same-session duplicates into one durable key while preserving evidence_refs, source_sessions, and superseded_by provenance")
               (step s6 :logic "turn low-confidence semantic duplicate candidates into needs-human review artifacts instead of deleting or activating them")
               (step s7 :logic "apply knowledge_review_state overlay before default retrieval so superseded/historical/duplicate/stale/delete-candidate memories leave the active reasoning path without deletion")
               (step s8 :logic "keep needs-human out of default retrieval until adjudicated, because uncertain memory is evidence rather than active guidance")
               (step s9 :logic "suppress architecture-module noise for sensitive credential/secret/SSH/token retrieval intents unless explicitly scoped")
               (step s10 :logic "project search, beacon, insight, and memory responses into reviewable evidence")
               (step s11 :logic "accept intent_memory_candidate from master/workflow context and persist stable high-confidence user intent as memory:decision while keeping uncertain intent in review overlay or candidate artifacts"))
        :egress [kb_result memory_projection search_hits insight_summary intent_memory_record])
      (function project-registry
        :surface project-registry
        :entry [mission_project ProjectRegistry.resolve]
        :core ((step s1 :logic "resolve registered project root from explicit project_id, cwd, or target_project")
               (step s2 :logic "reject ambiguous or outside-root runtime paths before workstation spawn")
               (step s3 :logic "return project metadata usable by request, plan, context, and workstation surfaces"))
        :egress [project_root project_id requested_cwd_policy])
      (function data-residency-universe
        :surface data-residency-universe
        :entry [project-maturity-registry project-blueprint-registry service-runtime-universe regional-compliance-research]
        :core ((step s1 :logic "load data-region-partition-contract and classify each data-bearing project into cn/global hard partitions plus any global operating zones")
               (step s2 :logic "load xjp-platform-partition-contract so data-bearing apps bind to xjp-cn or xjp-global rather than declaring independent auth/secret/payment/router stacks")
               (step s3 :logic "require regional issuer, secret, storage, payment, router/model, and cross-region egress policy before a data-bearing project can claim M6")
               (step s4 :logic "project PCEA and CUTHUB region declarations into Universe summaries without promoting unimplemented runtime targets to deployment truth")
               (step s5 :logic "treat cross-region data movement as default-deny unless the movement matches an allowed egress category and records audit evidence"))
        :egress [data_residency_summary region_partition_diagnostics m6_data_bearing_gate])
      (function board-frontend
        :surface board-frontend
        :entry [project-blueprint-registry board-blueprint.lisp packages/board]
        :core ((step s1 :logic "register Board's project-local frontend Lisp SSOT without folding it into the backend V3 monolith")
               (step s2 :logic "require Board frontend pillar-flow, implementation-map, and runtime-projection checkers")
               (step s3 :logic "pin frontend code surfaces to Lisp so later workstation shards receive disjoint write scopes")
               (step s4 :logic "project MissionD runtime slot/PTY state into UI code instead of stale static workstation lists"))
        :egress [frontend-blueprint-checks board-ui-surfaces runtime-projection-contract])
      (function mission-shared-memory
        :surface mission-shared-memory
        :entry [mission_shared_memory mission_context_slice mission_claim_status mission_task_delegate mission_swarm_run EventBus]
        :core ((step s1 :logic "append durable shared_events with idempotency, stream sequence, correlation, project/task/agent fields, and EventBus wakeup projection")
               (step s2 :logic "store shared_artifacts by content hash so investigation reports, context packs, diagnostics, and accepted shards dedupe across agents")
               (step s3 :logic "grant shared_claims leases for file/region/surface/test/db-migration write scopes with TTL, heartbeat, conflict, release, and expiry semantics")
               (step s4 :logic "track agent_cursors per stream so resident master and workers resume after restart without rereading whole ledgers")
               (step s5 :logic "serve mission_context_slice from compiled semantic IR plus task/shard artifacts so agents read compact facts before full Lisp")
               (step s6 :logic "treat .missiond/tasks/**/shared-memory.lisp as compatibility projection only; concurrent truth lives in Rust/Postgres shared memory")
               (step s7 :logic "status snapshots must expire stale active leases before counting activeClaims/staleClaims so master/load diagnostics are based on current coordination truth rather than pre-reap residue")
               (step s8 :logic "project mission_shared_memory action=evidence_view per evidence-governance-policy: task_result_artifacts as canonical worker output, conversations as provider/user read model, event_log/shared_events as causality, KB entries with knowledge_review_state as reviewed long-term knowledge, and BoardTask state as coordination projection, returned as named evidence lanes with authority order"))
        :egress [shared-event shared-artifact shared-claim agent-cursor context-slice EventBus-signal evidence_governance_view evidence_lane_summary authority_order])
      (function evidence-governance-view
        :surface evidence-governance-view
        :entry [mission_shared_memory.evidence_view task_result_artifacts conversations event_log shared_events knowledge_review_state board_tasks]
        :core ((step s1 :logic "load evidence-governance-policy to fix the authority order across worker output, conversation read model, event causality, reviewed KB, and Board coordination")
               (step s2 :logic "query task_result_artifacts as canonical worker and workflow outputs for the task or project")
               (step s3 :logic "query conversations as provider/user turn read models, preserving durable transcript metadata without treating it as completion authority")
               (step s4 :logic "query event_log and shared_events as the causal timeline for Board, runtime, external service, and worker events")
               (step s5 :logic "query knowledge with knowledge_review_state so only reviewed long-term knowledge is presented as memory evidence")
               (step s6 :logic "return named evidence lanes with role labels so Memory/KB, Logs, Timeline, Conversation, and Board do not invent competing final-result authorities"))
        :egress [evidence_governance_view evidence_lane_summary authority_order]))

    (pillar communication
      (function conversation-ingestion
        :surface conversation-ingestion
        :entry [mission_conversation_query mission_conversation_analyze mission_conversation_reconcile mission_timeline mission_retrospective_manage mission_embedding_ops]
        :core ((step s1 :logic "load conversation-ingestion-policy for read-model default and max limits")
               (step s2 :logic "ingest or query conversation/session/timeline records by project scope")
	               (step s3 :logic "when mission_conversation_query list is scoped by taskId and conversationType is omitted, query all provider conversation rows for that BoardTask by direct conversations.task_id plus message-anchored BoardTask id fallback, order direct bindings before message-only anchors and newest sessions before older sessions, so Claude/Codex/Gemini durable logs remain first-class evidence even after a reused slot is rebound to a later task without returning stale parent/subagent sessions first")
               (step s4 :logic "compaction timeline reconstruction tolerates legacy NULL started_at/message_count rows by coalescing before tuple decode")
               (step s5 :logic "derive bounded analysis_context from calibrated turns, message_labels, and task-result-artifacts without treating worker prompts as user intent")
               (step s6 :logic "surface conversation label calibration overlays and dry-run reports before any role or turn backfill applies")
               (step s7 :logic "derive analysis, reconciliation, retrospective, and embedding work items")
               (step s8 :logic "surface durable facts for context assembly and later memory projection"))
        :egress [conversation_rows timeline_events retrospective_result embedding_jobs])
      (function eventbridge
        :surface eventbridge
        :entry [/webhooks/service-event /webhooks/auth-event /webhooks/deploy-center-event mission_timeline.wait deployment-event-response project-registry-reconciliation]
        :core ((step s1 :logic "validate external webhook token and parse missiond.event-envelope.v1 or supported legacy service/auth event payloads")
               (step s2 :logic "preserve event identity, project_id, service_id, correlation_id, trace_id, authority, privacy_class, and source payload under ExternalServiceEvent payload._envelope")
               (step s3 :logic "publish SystemEvent::ExternalServiceEvent through the EventBus with deterministic service_id/event_id dedupe")
               (step s4 :logic "allow bounded mission_timeline waits by service_id, event_kind, project_id, and correlation_id so deployment and worker orchestration can advance from durable events instead of PTY polling")
               (step s5 :logic "trigger deployment-event-response and project-registry-reconciliation workflows while keeping deploy-center as deployment fact authority and Forge as catalog authority"))
        :egress [ExternalServiceEvent event_log mission_timeline_wait deploy_ops_boardtask project_reconcile_report])
      (function eventhub-service
        :surface eventhub-service-boundary
        :entry [EventBus-publish local-event-spool xjp-eventhub.wait xjp-eventhub.subscribe mission_timeline.wait]
        :core ((step s1 :logic "keep MissionD local EventBus authoritative for local agent/Board/slot/workflow wakeups")
               (step s2 :logic "classify selected local and external service events into missiond.event-envelope.v1")
               (step s3 :logic "relay selected events to xjp-eventhub through local-event-spool with idempotency, privacy redaction, retry, and dead-letter state")
               (step s4 :logic "consume xjp-eventhub waits/subscriptions for cross-service deploy/auth/router/timeline events")
               (step s5 :logic "fallback to local event_log wait only with visible diagnostic when xjp-eventhub is unavailable"))
        :egress [EventHubEvent local-event-spool-diagnostic mission_timeline_wait])
      (function interaction-gateway
        :surface interaction-gateway
        :entry [/interactions/v1/messages /interactions/v1/{interaction_id}/events /jarvis/v1/chat/completions mission_interaction InteractionEnvelope channel-adapter]
        :core ((step s1 :logic "normalize Web, iOS, Jarvis, WeChat bridge, and service-trigger input into missiond.interaction-envelope.v1; legacy OpenAI-compatible /v1/chat/completions must use handle_chat_completions_interaction_adapter/openai_request_to_interaction_envelope and must not directly write provider PTYs")
               (step s2 :logic "resolve identity through Auth: bearer JWT for Web/iOS/Jarvis MUST call Auth /oidc/userinfo or configured MISSIOND_INTERACTION_AUTH_USERINFO_URL; service tokens use a separate internal service context and smoke automation must inject MISSIOND_INTERACTION_SERVICE_TOKEN from secret-store ref missiond.jarvis-smoke/INTERACTION_SERVICE_TOKEN or let scripts/smoke-jarvis-interaction.mjs read that same ref through MISSIOND_JARVIS_SMOKE_SECRET_REF; deploy persists interaction auth env keys into launchd only when supplied by the operator environment; wechat_openid/unionid without binding returns identity_binding_required; Auth timeout or non-2xx response returns INTERACTION_AUTH_UNAVAILABLE and must not create BoardTask side effects")
               (step s3 :logic "derive PermissionContext from Auth userinfo with user_id, tenant_id, application_id, product_id, groups, roles, channel, and capabilities before any BoardTask or worker side effect; request metadata may only provide hints, not authority")
               (step s4 :logic "run mission_context_gather(persist=true) from explicit unknowns and bind grounding_context_id plus sources_used to the interaction")
               (step s5 :logic "write intent_draft artifact and require user/channel confirmation for broad human requests")
               (step s6 :logic "write plan_draft artifact and require confirmation before BoardTask creation unless the caller supplies an exact workflow/exact shard policy gate")
               (step s7 :logic "create BoardTask metadata with interaction_id, PermissionContext, grounding_context_id, intent_artifact_id, plan_artifact_id, and dispatch contract")
               (step s8 :logic "stream response sink events for status, BoardTask, worker, task-result-artifact, final, and typed diagnostics; PTY output remains diagnostic only"))
        :egress [InteractionEventStream PermissionContext grounding_context_id intent_artifact plan_artifact BoardTask task-result-artifact response-sink interaction-audit])
      (function interaction-ledger
        :surface interaction-ledger
        :entry [InteractionEnvelope SSEEvent conversation_events shared_artifacts BoardTask.runtime_metadata task-result-artifact]
        :core ((step s1 :logic "correlate each client-channel request to one stable interaction_id and scoped conversation_id")
               (step s2 :logic "persist every user-visible interaction lifecycle event into conversation_events as interaction.* raw_data before or alongside channel projection")
               (step s3 :logic "bind grounding, intent, plan, BoardTask, worker conversation, and task-result-artifact ids into runtime_metadata or event raw_data")
               (step s4 :logic "replay /interactions/v1/{interaction_id}/events from durable ledger rows in insertion order instead of static placeholder text")
               (step s5 :logic "merge messages, interaction events, shared artifacts, and BoardTask metadata into a conversation-control-plane replay view"))
        :egress [interaction-event-stream conversation-replay interaction-audit-view task-result-artifact])
      (function router-policy
        :surface router-policy
        :entry [mission_router_chat mission_router_chat_manage router-policy-dry-run]
        :core ((step s1 :logic "load advisory router policy, backend readiness, and dispatch descriptors")
               (step s2 :logic "run recommendation or chat routing in dry-run/advisory mode unless an explicit later gate allows apply")
               (step s3 :logic "emit measurable descriptors without replacing the runtime backend"))
        :egress [router_recommendation backend_readiness dispatch_descriptor])
      (function incident-question-governance
        :surface incident-governance
        :entry [mission_question mission_llm_trace mission_decision_stats mission_gemini_auth mission_incident]
        :core ((step s1 :logic "record question, trace, decision, auth, or incident facts with deterministic ids")
               (step s2 :logic "revalidate operational questions before display/listing; if runtime evidence proves the issue stale or already fixed, answer with stale_evidence/resolved_by_runtime_fix and emit QuestionEvent::Resolved")
               (step s3 :logic "route still-valid blocked work to the human/orchestrator without mutating the primary artifact")
               (step s4 :logic "return status, revalidation hints, and unblock hints to Autopilot or request-flow callers"))
        :egress [question_id incident_record decision_stats question_revalidation_status unblock_hint])
      (function decision-inbox-revalidation
        :surface decision-inbox-revalidation
        :entry [mission_question_list mission_question_get mission_master_status runtime-status-evidence]
        :core ((step s1 :logic "classify pending questions by revalidator kind such as lisp-code-sync self-loop, MCP reconnect, deploy event, or generic user decision")
               (step s2 :logic "for operational revalidators, read authoritative runtime status before showing the item to the user")
               (step s3 :logic "mark stale evidence as answered with stale_evidence/resolved_by_runtime_fix, emit QuestionEvent::Resolved, and close linked operational BoardTask as done/stale when the question itself was the blocker; only generic answered questions use normal unblock semantics")
               (step s4 :logic "show still-valid questions with revalidationStatus/evidenceFreshAt so the frontend can distinguish fresh decisions from stale accidents"))
        :egress [question_revalidation_status stale_evidence_answer QuestionEvent::Resolved])
      (function capability-governance
        :surface capability-governance
        :entry [mission_capability_usage mission_audit mission_codex_ops mission_tool_directory mission_agent_navigation]
        :core ((step s1 :logic "load capability-governance-policy and mcp-tool-governance-policy for review sidecar path, protected source/target lists, and primary tool families")
               (step s2 :logic "record capability usage, audit facts, and Codex operation acknowledgements")
               (step s3 :logic "bind evidence to plan/execution ids without becoming the primary execution gate")
               (step s4 :logic "map natural-language operator intent to a primary tool family before raw MCP tool selection")
               (step s5 :logic "lookup concrete MCP tool names, return preferred family/surface, and explain compatibility tools as leaves under the smaller family catalog")
               (step s6 :logic "return traceable receipts and tool family/recommendation/deprecated hints for later learning and report finalization"))
        :egress [capability_receipt audit_record codex_ops_result tool_family_directory tool_recommendation deprecated_tool_hint]))

    (pillar worker-runtime
      (function compute-primitives
        :surface compute-primitives
        :entry [mission_task_submit mission_task_query mission_task_cancel mission_job_poll mission_flow_run mission_pty_spawn mission_pty_send mission_pty_read mission_pty_signal mission_pty_confirm mission_pty_status mission_pty_screenshot mission_slots mission_slot_history mission_agent mission_inbox mission_sonnet_process mission_minimax_process mission_cc_query mission_cc_swarm mission_worker mission_control mission_pause mission_forge_build mission_forge_lint CodexCliStateParser GeminiCliUpstreamStateParser recognize_screen]
        :core ((step s1 :logic "normalize low-level runtime requests into slot, job, task, PTY, flow, forge, or process operations")
               (step s2 :logic "apply project-root, permission, timeout, and pause/control policies before side effects")
               (step s3 :logic "for mission_task_query status/list, read both legacy tasks and BoardTask-backed delegated workers so running BoardTasks are visible to master/control callers")
               (step s4 :logic "classify provider PTY text into running/idle/blocked/complete/unknown with confidence and reason")
               (step s5 :logic "return durable runtime handles and status without bypassing BoardTask or plan execution when a higher-level surface exists"))
        :egress [runtime_handle job_status pty_snapshot PtyRecognitionSnapshot flow_result forge_result])
      (function skill-runtime
        :surface skill-runtime
        :entry [mission_skill_query mission_skill_context mission_skill_mutate mission_skill_exec]
        :core ((step s1 :logic "resolve skill metadata and context through the skill registry")
               (step s2 :logic "clean-machine skill SQL counter reads MUST cast integer counters to bigint before decoding into SkillTopic i64 fields")
               (step s3 :logic "for operational questions such as remote hosts, deploy-agent, router embedding/rerank, 12900kf, Windows runner, CI runner, Ollama, or model host, extract bounded skill-derived operational_facts with source_path/source_line before ad-hoc probing")
               (step s4 :logic "validate mutation or execution request against project and permission policy")
               (step s5 :logic "for requires_approval skill workflows, return a pending_approval preview first and require a second explicit approve=true call after human review before executing")
               (step s6 :logic "return skill execution result, operational fact bundle, or context bundle as a runtime receipt"))
        :egress [skill_context skill_operational_facts skill_mutation skill_execution_receipt])
      (function cascade-governance
        :surface cascade-governance
        :entry [mission_universe_graph mission_cascade_plan mission_cascade_trigger mission_cascade_lint]
        :core ((step s1 :logic "read universe graph and cascade plan inputs")
               (step s2 :logic "lint or trigger cascades through explicit control-tree/governance policy")
               (step s3 :logic "return plan/lint/trigger result for orchestrator review"))
        :egress [universe_graph cascade_plan cascade_lint cascade_trigger_result]))

    (pillar operations
      (function sysinfra-control
        :surface sysinfra-control
        :entry [mission_infra_query mission_infra_ops mission_permission_query mission_permission_mutate mission_power_control mission_sys_logs mission_sys_config mission_daemon_update mission_global_instruction]
        :core ((step s1 :logic "normalize sysinfra, permission, power, daemon, and global instruction operations")
               (step s2 :logic "for mission_daemon_update, require explicit confirm=true, return an async logged deploy job for full builds, and reserve synchronous restart for skip_build")
               (step s3 :logic "enforce explicit side-effect policy and keep operational state separate from request artifacts")
               (step s4 :logic "merge configured servers with skill-derived operational host facts, including windows-runner/12900kf deploy-agent and embedding anchors, until deploy-center owns the authoritative runtime record")
               (step s5 :logic "return bounded operational status or mutation receipt"))
        :egress [infra_result permission_receipt daemon_update_status global_instruction_state])
      (function runtime-load-explanation
        :surface runtime-load-explanation
        :entry [mission_master_status mission_health daemon_stats lispCodeSync sharedMemory nightlyEvolution]
        :core ((step s1 :logic "collect daemon counter snapshots for EventBus backlog, DB latency, Autopilot latency, context prefetch, lisp-code-sync reports, shared-memory workflow runs, stale claims, and nightly evolution")
               (step s2 :logic "classify likely internal load sources into lisp-code-sync, eventbus, shared-memory/workflow-runner, autopilot/db, context-prefetch, or nightly-evolution instead of asking the operator to infer from raw CPU")
               (step s3 :logic "state the limitation that this is internal daemon attribution and OS top/Activity Monitor is still needed for process-level CPU confirmation")
               (step s4 :logic "return runtimeLoadExplanation alongside mission_master_status so stale decisions and frontend diagnostics can cite one authoritative explanation"))
        :egress [runtimeLoadExplanation load_suspects diagnostic_limits])
      (function ops-infra
        :surface ops-infra
        :entry [scripts/deploy-daemon.sh scripts/rustfmt-missiond.sh scripts/cargo-fmt-touched.sh]
        :core ((step s1 :logic "build, backup, codesign, install, kickstart, and smoke daemon as one command")
	               (step s2 :logic "retry IPC initialize smoke after socket readiness and rollback on real failure")
	               (step s3 :logic "prove MissionD-owned crates are formatter-converged through rustfmt-missiond; use cargo-fmt-touched only for external or non-M6 scoped waves")
	               (step s4 :logic "keep restart-time background indexing event-driven unless an operator explicitly opts into repository-wide AST full sync"))
        :egress [deployed-daemon rollback-result formatter-convergence-result scoped-rustfmt-result])
      (function missiond-blue-green-self-update
        :surface missiond-blue-green-self-update
        :entry [scripts/deploy-daemon.sh mission_daemon_update release-manifest cleanup rollback]
        :core ((step s1 :logic "compile typed Lisp runtime projections before building release candidates")
               (step s2 :logic "build immutable paired missiond/mission-mcp release directories and write release-manifest.json")
               (step s3 :logic "switch the active symlink only after pre-switch MCP smoke and post-switch daemon IPC smoke pass")
               (step s4 :logic "support previous-release rollback, cleanup-only dry-run/apply, and incomplete release removal without mutating active on failure"))
        :egress [release-manifest active-symlink rollback-result cleanup-report typed_lisp_runtime])))
