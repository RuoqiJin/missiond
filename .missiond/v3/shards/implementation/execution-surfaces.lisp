(implementation-map
(surface evidence-collector
      :status "code-aligned"
      :implements [verification-receipt]
      :code ["crates/missiond-daemon/src/handlers/knowledge/evidence_collector.rs"
             "crates/missiond-daemon/src/handlers/knowledge/evidence_collector/append.rs"
             "crates/missiond-daemon/src/handlers/knowledge/evidence_collector/entry.rs"
             "crates/missiond-daemon/src/handlers/knowledge/evidence_collector/event_ref.rs"
             "crates/missiond-daemon/src/handlers/knowledge/evidence_collector/legacy.rs"
             "crates/missiond-daemon/src/handlers/knowledge/evidence_collector/resolver.rs"
             "crates/missiond-daemon/src/handlers/knowledge/evidence_collector/taxonomy.rs"
             "crates/missiond-daemon/src/handlers/knowledge/evidence_collector/tests.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan/evidence_sidecar.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan/execution_runtime.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan/execution_runtime/internal.rs"
             "scripts/check-v3-evidence-collector-isomorphism.mjs"]
      :note "evidence_collector.rs remains the compatibility facade for the verification-receipt evidence surface. evidence_collector/taxonomy.rs owns EVIDENCE_SCHEMA_VERSION and source/kind wire constants; evidence_collector/event_ref.rs owns EventRefStatus live | log | unavailable plus EventRefProvenance live | passive_cache | event_log_query | unavailable; evidence_collector/entry.rs owns EvidenceEntry typed builder/projection; evidence_collector/append.rs owns AppendOutcome and the sidecar append writer; evidence_collector/legacy.rs owns wrap_legacy_record_evidence. evidence_collector/resolver.rs owns EventRefResolver, EVENT_REF_CACHE_CAP = 1024, cache-miss/log-query miss constants, and the bounded event-log query recovery path. wrap_legacy_record_evidence lifts caller-supplied JSON evidence into the typed EvidenceEntry envelope without losing prior fields, keeping plan.rs com..."
      :evidence-sidecar ".missiond/v3/evidence/blueprint-notes.lisp#note-005")

(surface mission_execution-log
	      :status "code-aligned"
	      :implements [execution-lifecycle execution-event-bus session-trace]
	      :code ["crates/missiond-daemon/src/handlers/knowledge/agent_execution.rs"
	             "crates/missiond-daemon/src/handlers/knowledge/agent_execution/tests.rs"
	             "crates/missiond-daemon/src/handlers/knowledge/agent_execution/log_surface.rs"
	             "crates/missiond-daemon/src/handlers/knowledge/agent_execution/log_dispatch.rs"
	             "crates/missiond-daemon/src/handlers/knowledge/agent_execution/log_governance.rs"
	             "crates/missiond-daemon/src/handlers/knowledge/agent_execution/log_deviation.rs"
	             "crates/missiond-daemon/src/handlers/knowledge/agent_execution/log_decision.rs"
	             "crates/missiond-daemon/src/handlers/knowledge/agent_execution/log_issue.rs"
	             "crates/missiond-daemon/src/handlers/knowledge/agent_execution/log_open.rs"
	             "crates/missiond-daemon/src/handlers/knowledge/agent_execution/log_list.rs"
	             "crates/missiond-daemon/src/handlers/knowledge/agent_execution/log_status.rs"
	             "crates/missiond-daemon/src/handlers/knowledge/agent_execution/log_counters.rs"
	             "crates/missiond-daemon/src/handlers/knowledge/agent_execution/log_store.rs"
	             "crates/missiond-daemon/src/handlers/knowledge/agent_execution/log_mutation.rs"
	             "crates/missiond-daemon/src/handlers/knowledge/agent_execution/log_paths.rs"
	             "crates/missiond-daemon/src/handlers/knowledge/agent_execution/log_template.rs"
	             "crates/missiond-daemon/src/handlers/knowledge/agent_execution/lisp_syntax.rs"
	             "crates/missiond-daemon/src/handlers/knowledge/agent_execution/lisp_syntax_node.rs"
	             "crates/missiond-daemon/src/handlers/knowledge/agent_execution/lisp_syntax_balance.rs"
	             "crates/missiond-daemon/src/handlers/knowledge/agent_execution/session_trace.rs"
	             "crates/missiond-daemon/src/handlers/knowledge/agent_execution/session_trace_event.rs"
	             "crates/missiond-mcp/src/tools/knowledge/agent_execution.rs"
	             "scripts/check-v3-mission-execution-isomorphism.mjs"]
	      :note "mission_execution-log is the durable companion-log and live-projection surface for mission_execution. agent_execution/log_surface.rs keeps emit_execution_event plus compatibility re-exports after durable writes succeed; split log modules own paths, storage, mutation, dispatch metadata, counters, status read-model, and session-trace projection."
	      :evidence-sidecar ".missiond/v3/evidence/blueprint-notes.lisp#note-006")

(surface mission_execution-claim-lease
	      :status "code-aligned"
	      :implements [execution-claim-lease scoped-write-gate]
	      :code ["crates/missiond-daemon/src/handlers/knowledge/agent_execution.rs"
	             "crates/missiond-daemon/src/handlers/knowledge/agent_execution/tests.rs"
	             "crates/missiond-daemon/src/handlers/knowledge/agent_execution/claim_heartbeat.rs"
	             "crates/missiond-daemon/src/handlers/knowledge/agent_execution/claim_lease.rs"
	             "crates/missiond-daemon/src/handlers/knowledge/agent_execution/claim_records.rs"
	             "crates/missiond-daemon/src/handlers/knowledge/agent_execution/claim_release.rs"
	             "crates/missiond-mcp/src/tools/knowledge/agent_execution.rs"
	             "scripts/check-v3-mission-execution-isomorphism.mjs"]
	      :note "mission_execution-claim-lease owns the conflict window around execution work. agent_execution/claim_lease.rs owns DEFAULT_LEASE_SECS = 1800, MAX_LEASE_SECS = 24 * 3600, scopes_overlap, scopes_overlap_pure, action_claim, and compatibility re-exports. agent_execution/claim_records.rs owns ClaimRecord, parse_claims, parse_iso, and find_claim_node for active/released claim read-model projection. agent_execution/claim_heartbeat.rs owns action_heartbeat and lease-expires-at extension. agent_execution/claim_release.rs owns action_release and released-at/status projection. scopes_overlap_pure is re-exported for the Plan DAG scheduler and scoped-commit checks so claim overlap, staged path checks, and released-claim handoff all use one rule.")

(surface mission_execution-completion-audit
	      :status "code-aligned"
	      :implements [execution-completion scoped-commit-handoff task-run-auto-verifier]
	      :code ["crates/missiond-daemon/src/handlers/knowledge/agent_execution.rs"
	             "crates/missiond-daemon/src/handlers/knowledge/agent_execution/tests.rs"
	             "crates/missiond-daemon/src/handlers/knowledge/agent_execution/completion_contract_gate.rs"
	             "crates/missiond-daemon/src/handlers/knowledge/agent_execution/completion_audit.rs"
	             "crates/missiond-daemon/src/handlers/knowledge/agent_execution/completion_audit_findings.rs"
	             "crates/missiond-daemon/src/handlers/knowledge/agent_execution/completion_entry.rs"
	             "crates/missiond-daemon/src/handlers/knowledge/agent_execution/completion_fields.rs"
	             "crates/missiond-daemon/src/handlers/knowledge/agent_execution/completion_handoff_audit.rs"
	             "crates/missiond-daemon/src/handlers/knowledge/agent_execution/completion_id_audit.rs"
	             "crates/missiond-daemon/src/handlers/knowledge/agent_execution/completion_indexes.rs"
	             "crates/missiond-daemon/src/handlers/knowledge/agent_execution/completion_inputs.rs"
	             "crates/missiond-daemon/src/handlers/knowledge/agent_execution/completion_maintenance.rs"
	             "crates/missiond-daemon/src/handlers/knowledge/agent_execution/completion_repair.rs"
	             "crates/missiond-daemon/src/handlers/knowledge/agent_execution/completion_records.rs"
	             "crates/missiond-daemon/src/handlers/knowledge/agent_execution/completion_durability.rs"
	             "crates/missiond-daemon/src/handlers/knowledge/agent_execution/completion_response.rs"
	             "crates/missiond-daemon/src/handlers/knowledge/agent_execution/completion_gates.rs"
	             "crates/missiond-daemon/src/handlers/knowledge/agent_execution/completion_trace.rs"
	             "crates/missiond-daemon/src/handlers/knowledge/agent_execution/completion_verification.rs"
	             "crates/missiond-daemon/src/handlers/knowledge/agent_execution/preflight.rs"
	             "crates/missiond-daemon/src/handlers/knowledge/agent_execution/preflight_contract.rs"
	             "crates/missiond-daemon/src/handlers/knowledge/agent_execution/preflight_contract_scope.rs"
	             "crates/missiond-daemon/src/handlers/knowledge/agent_execution/preflight_cwd.rs"
	             "crates/missiond-daemon/src/handlers/knowledge/agent_execution/preflight_trace.rs"
	             "crates/missiond-daemon/src/handlers/knowledge/agent_execution/preflight_patterns.rs"
	             "crates/missiond-daemon/src/handlers/knowledge/agent_execution/preflight_porcelain.rs"
	             "crates/missiond-daemon/src/handlers/knowledge/agent_execution/preflight_scope.rs"
	             "crates/missiond-daemon/src/handlers/knowledge/agent_execution/task_verifier.rs"
	             "crates/missiond-daemon/src/handlers/knowledge/agent_execution/task_verifier_auto.rs"
	             "crates/missiond-daemon/src/handlers/knowledge/agent_execution/task_verifier_auto_artifacts.rs"
	             "crates/missiond-daemon/src/handlers/knowledge/agent_execution/task_verifier_inputs.rs"
	             "crates/missiond-daemon/src/handlers/knowledge/agent_execution/task_verifier_preconditions.rs"
	             "crates/missiond-daemon/src/handlers/knowledge/agent_execution/task_verifier_report.rs"
	             "crates/missiond-mcp/src/tools/knowledge/agent_execution.rs"
	             "scripts/check-v3-mission-execution-isomorphism.mjs"]
	      :note "mission_execution-completion-audit owns the completion durability gate. action_complete records completion facts from the facade, while agent_execution/completion_fields.rs owns VALID_COMMIT_STATUSES, VALID_VERIFIER_STATUSES, VALID_TASK_RUN_VERIFIER_STATUSES, normalize_commit_status, normalize_verifier_status, normalize_task_run_verifier_status, collect_string_list, render_string_list, parse_string_list, and the commit-status-without-hash / commit-status-blocked-without-blocker / scoped-commit-violation finding constants."
	      :evidence-sidecar ".missiond/v3/evidence/blueprint-notes.lisp#note-007")

(surface mission_workflow
      :status "code-aligned"
      :implements [workflow workflow-distiller]
      :code ["crates/missiond-daemon/src/handlers/knowledge/workflow.rs"
             "crates/missiond-daemon/src/handlers/knowledge/workflow/artifacts.rs"
             "crates/missiond-daemon/src/handlers/knowledge/workflow/auto_chain.rs"
             "crates/missiond-daemon/src/handlers/knowledge/workflow/auto_chain/recorder.rs"
             "crates/missiond-daemon/src/handlers/knowledge/workflow/auto_chain/rules.rs"
             "crates/missiond-daemon/src/handlers/knowledge/workflow/auto_sonnet.rs"
             "crates/missiond-daemon/src/handlers/knowledge/workflow/auto_sonnet/policy.rs"
             "crates/missiond-daemon/src/handlers/knowledge/workflow/compile_methodology.rs"
             "crates/missiond-daemon/src/handlers/knowledge/workflow/distill.rs"
             "crates/missiond-daemon/src/handlers/knowledge/workflow/methodology.rs"
             "crates/missiond-daemon/src/handlers/knowledge/workflow/methodology/extract.rs"
             "crates/missiond-daemon/src/handlers/knowledge/workflow/methodology/io.rs"
             "crates/missiond-daemon/src/handlers/knowledge/workflow/methodology/source.rs"
             "crates/missiond-daemon/src/handlers/knowledge/workflow/methodology/types.rs"
             "crates/missiond-daemon/src/handlers/knowledge/workflow/methodology/yaml.rs"
             "crates/missiond-daemon/src/handlers/knowledge/workflow/project_root.rs"
             "crates/missiond-daemon/src/handlers/knowledge/workflow/review_resolution.rs"
             "crates/missiond-daemon/src/handlers/knowledge/workflow/run_methodology.rs"
             "crates/missiond-daemon/src/handlers/knowledge/workflow/store_actions.rs"
             "crates/missiond-daemon/src/handlers/knowledge/workflow/tests.rs"
             "crates/missiond-mcp/src/tools/knowledge/workflow.rs"]
      :model-projection "mission_workflow sonnet distiller compiler_model labels project from router-runtime-policy queued_sonnet_model through RouterRuntimeConfig; local Rust model literals are forbidden on this production path."
      :note "workflow.rs remains the thin mission_workflow action facade. workflow/store_actions.rs owns list/get/match/apply/record_execution and parse_id_arg; workflow/project_root.rs owns the canonical project-root resolver and the no process-cwd fallback invariant; workflow/compile_methodology.rs owns CompileMode, parse_compile_mode, action_compile_methodology, dry-run preview, deterministic YAML compile, methodology V3 artifact projection, review-gate receipt emission, and count_top_form; workflow/run_methodology.rs owns compiled YAML resolution, mission_flow_run dispatch, parse_run_methodology_record_intent, and methodology_execution_record_payload. workflow/distill.rs owns DistillMode, parse_distill_mode, action_distill, action_distill_dry_run, action_distill_sonnet, evidence sidecar path/read/gate, workflow_sexp JSON extraction, balanced-S-expression validation, name-refer..."
      :evidence-sidecar ".missiond/v3/evidence/blueprint-notes.lisp#note-008")

(surface work-order-lifecycle
      :status "code-aligned"
      :implements [work-order-intent work-order-plan work-order-audit board-intent-unification external-app-delegation task-result-artifact-projection]
      :code ["crates/missiond-daemon/src/handlers/knowledge/request.rs"
             "crates/missiond-daemon/src/handlers/knowledge/request/respond/materialization.rs"
             "crates/missiond-daemon/src/handlers/knowledge/file_artifacts.rs"
             "crates/missiond-daemon/src/handlers/knowledge/board/create.rs"
             "crates/missiond-daemon/src/handlers/knowledge/board/events.rs"
             "crates/missiond-daemon/src/handlers/knowledge/workflow.rs"
             "crates/missiond-daemon/src/engine/shared_memory.rs"
             "crates/missiond-daemon/src/handlers/compute/task_delegate.rs"
             "crates/missiond-mcp/src/tools/knowledge/request.rs"
             "crates/missiond-mcp/src/tools/knowledge/board.rs"
             "crates/missiond-mcp/src/tools/knowledge/workflow.rs"
             "crates/missiond-mcp/src/tools/knowledge/shared_memory.rs"
             ".missiond/workflows/work-order-lifecycle.lisp"
             "scripts/missiond-work-order.mjs"
             ".githooks/pre-commit"
             "scripts/hooks/pre-commit-missiond-work-order"
             "scripts/check-v3-work-order-lifecycle-isomorphism.mjs"]
      :acceptance ["node scripts/check-v3-work-order-lifecycle-isomorphism.mjs --json"
                   "node scripts/check-v3-pillar-flow-schema.mjs --engine=ocaml --json"]
      :note "work-order-lifecycle is the single governance chain for user requests, Board-triggered worker tasks, intent.lisp files, and external application delegation such as translation or code-refactor jobs. It intentionally reuses mission_request, mission_board, mission_workflow, mission_shared_memory, and file_artifacts instead of adding another public MCP tool family: every source is normalized into work-order intent, bound to one BoardTask, compiled into plan.lisp accepted shards, executed through workflow_run/shared-memory, and closed by task-result-artifacts plus audit.lisp. Board notes and provider finals remain projections; external apps get the same audit/replay semantics without learning MissionD internals.")

(surface external-work-order-gate
      :status "code-aligned"
      :implements [work-order-start work-order-staged-verify work-order-commit-verify precommit-work-order-gate ci-work-order-gate code-first-drift-backfill]
      :code ["scripts/missiond-work-order.mjs"
             ".githooks/pre-commit"
             "scripts/hooks/pre-commit-missiond-work-order"
             ".missiond/workflows/work-order-lifecycle.lisp"
             "scripts/check-v3-work-order-lifecycle-isomorphism.mjs"]
      :acceptance ["node scripts/check-v3-work-order-lifecycle-isomorphism.mjs --json"]
      :note "external-work-order-gate is the engineering boundary for external Codex/ClaudeCode/user-local code changes. It does not assume the agent read a prompt file: local hooks, commit trailers, and CI/deploy verification require a MissionD-Work-Order id, intent.lisp, plan.lisp, accepted_shard_id, and write_scope coverage before accepting code changes. Code-first changes that bypass the gate become visible drift backfill tasks rather than silent accepted work.")

(surface task-runner-cli
      :status "code-aligned"
      :implements [execution-lifecycle verification-receipt final-report]
      :code ["scripts/task-runner-next-action.mjs"
             "scripts/task-runner-dispatch.mjs"
             "scripts/task-runner-submit-dispatch.mjs"
             "scripts/check-task-lifecycle-events.mjs"
             "scripts/task-runner-append-event.mjs"
             "scripts/task-runner-finalize-report.mjs"
             "scripts/task-runner-parent-hotfix.mjs"
             "scripts/project-task-lifecycle-ledger.mjs"
             "crates/missiond-daemon/src/handlers/knowledge/plan/task_runner_dry_run.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan/task_runner_dry_run/manifest.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan/task_runner_dry_run/projection.rs"
             "scripts/check-verification-receipt.mjs"
             "scripts/verify-task-contract.mjs"
             "scripts/verify-task-runner-batch.mjs"]
      :note "Task-scoped lifecycle events are first-class one-event files: the primary task-scoped path is .missiond/tasks/<wave>/events/<seq>.event.lisp (one lifecycle-event form per file, schema=missiond.task-lifecycle-event.v1, validated by check-task-lifecycle-events as standalone task-scoped event files), and task-runner-append-event allocates the next numeric file under a directory lock, validates the candidate bytes, and atomically creates them via fs.openSync(file, 'wx') when --events-dir is supplied. The legacy task-scoped task-lifecycle-events.lisp ledger is now a compatibility projection/input only: existing --ledger callers keep working unchanged, and task-runner-wave-state reads conventional task-scoped event files when present and falls back to the legacy ledger for historical waves, deduping by event id when both inputs exist."
      :evidence-sidecar ".missiond/v3/evidence/blueprint-notes.lisp#note-010")

(surface workstation-dispatch
      :status "code-aligned"
      :implements [workstation-dispatch substrate-dispatch audit-dispatch]
      :code ["crates/missiond-daemon/src/handlers/knowledge/workstation_dispatch.rs"
             "crates/missiond-daemon/src/handlers/knowledge/workstation_dispatch/descriptor.rs"
             "crates/missiond-daemon/src/handlers/knowledge/workstation_dispatch/decision.rs"
             "crates/missiond-daemon/src/handlers/knowledge/workstation_dispatch/outcome.rs"
             "crates/missiond-daemon/src/handlers/knowledge/workstation_dispatch/runner.rs"
             "crates/missiond-daemon/src/handlers/knowledge/workstation_dispatch/brief.rs"
             "crates/missiond-daemon/src/handlers/knowledge/workstation_dispatch/proposal.rs"
             "crates/missiond-daemon/src/handlers/knowledge/workstation_dispatch/auto_spawn.rs"
             "crates/missiond-daemon/src/handlers/knowledge/workstation_dispatch/auto_spawn/gate.rs"
             "crates/missiond-daemon/src/handlers/knowledge/workstation_dispatch/auto_spawn/hash.rs"
             "crates/missiond-daemon/src/handlers/knowledge/workstation_dispatch/auto_spawn/input.rs"
             "crates/missiond-daemon/src/handlers/knowledge/workstation_dispatch/auto_spawn/outcome.rs"
             "crates/missiond-daemon/src/handlers/knowledge/workstation_dispatch/tests.rs"
             "scripts/check-v3-request-flow-smoke.mjs"
             "scripts/check-v3-workstation-dispatch-isomorphism.mjs"]
      :auto-spawn-boundary "workstation_dispatch/auto_spawn.rs owns the true-spawn facade; auto_spawn/input.rs owns strict gate inputs; auto_spawn/hash.rs owns proposal hash projection; auto_spawn/outcome.rs owns WorkstationAutoSpawnGateOutcome and status wire projection; auto_spawn/gate.rs owns enforce_auto_spawn_preflight and evaluate_workstation_auto_spawn_gate."
      :brief-invariant "workstation_dispatch/brief.rs MUST render a visible 'forbidden git state mutations' bullet in the Commit policy section that names `git stash`, `git reset`, `git checkout`, and `git restore` and tells the worker to stop + note the BoardTask rather than mutate hidden worktree state. The test workstation_dispatch::tests::brief_forbids_hidden_git_state_mutations_unless_owned and the WORKSTATION_BRIEF_RS_NEEDLES + BLUEPRINT_SURFACE_BODY_ANCHORS entry 'forbidden git state mutations' pin this line so it cannot be silently dropped from the brief or the substrate contract."
      :anchors [run_workstation_dispatch_with_contract_and_trace classify_task_kind build_task_brief "proposal model label projects from router-runtime-policy queued_sonnet_model" extract_inner_board_task_id dry_run_no_dispatch DryRun Dispatched ParsedTaskContract InferenceContext SafeDescriptorReason BriefTaskKind parse_task_contract evaluate_dispatch_decision outcome_to_response_fields "forbidden git state mutations"]
      :note "workstation-dispatch is the substrate called by mission_plan execute_internal after target=mission_task_delegate is selected; workstation-config owns slot/model/prompt setup, while this surface owns the WorkstationDispatchOutcome response vocabulary and the handoff contract. The rendered brief MUST carry the forbidden git state mutations invariant (git stash / git reset / git checkout / git restore are off-limits unless the task contract explicitly owns the operation) so a delegated worker that meets a dirty worktree stops and adds a BoardTask note instead of silently rewinding shared state."
      :evidence-sidecar ".missiond/v3/evidence/blueprint-notes.lisp#note-013")
)
