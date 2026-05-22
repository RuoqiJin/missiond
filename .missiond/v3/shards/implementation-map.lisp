  (implementation-map
    (surface mission_request
      :status "code-aligned"
      :role "single user-facing request entry"
      :code ["crates/missiond-daemon/src/handlers/knowledge/request.rs"
             "crates/missiond-daemon/src/handlers/knowledge/request/request_artifacts.rs"
             "crates/missiond-daemon/src/handlers/knowledge/request/respond.rs"
             "crates/missiond-daemon/src/handlers/knowledge/request/respond/events.rs"
             "crates/missiond-daemon/src/handlers/knowledge/request/respond/materialization.rs"
             "crates/missiond-daemon/src/handlers/knowledge/request/respond/routing.rs"
             "crates/missiond-daemon/src/handlers/knowledge/request/review_packet.rs"
             "crates/missiond-daemon/src/handlers/knowledge/request/tests.rs"
             "crates/missiond-mcp/src/tools/knowledge/request.rs"]
      :note "V3 physical split: request.rs remains the mission_request action facade plus start/advance/status entry adapter, request/request_artifacts.rs owns request-local paths, request.lisp and lifecycle event rendering, projection planning, pipeline-meta extraction, compat opt-in policy helpers, and JSON artifact status projection, request/respond.rs owns review-response adapter orchestration: approve_intent/approve_plan/execute_plan delegation and blocked-response construction; request/respond/events.rs owns request-local review event sequencing/rendering and next_action projection; request/respond/materialization.rs owns hidden BoardTask anchor creation, request-local plan.lisp materialization/amendment, Plan row insertion, and materialization JSON projection; request/respond/routing.rs owns response parsing, directive/plan ref resolution, Lisp keyword ref scanning, and app..."
      :evidence-sidecar ".missiond/v3/evidence/blueprint-notes.lisp#note-001")

    (surface unified-entry-runtime
      :status "code-aligned"
      :implements [unified-entry-pipeline request-runtime-bridge]
      :code ["crates/missiond-daemon/src/handlers/knowledge/unified_entry.rs"
             "crates/missiond-daemon/src/handlers/knowledge/unified_entry/planner.rs"
             "crates/missiond-daemon/src/handlers/knowledge/unified_entry/decorator.rs"
             "crates/missiond-daemon/src/handlers/knowledge/unified_entry/stages.rs"
             "crates/missiond-daemon/src/handlers/knowledge/unified_entry/tests.rs"
             "crates/missiond-daemon/src/handlers/knowledge/request.rs"
             "scripts/check-v3-unified-entry-isomorphism.mjs"]
      :anchors [pipeline_stage flow_ref artifact_refs next_step mission_request mission_directive mission_plan mission_workflow]
      :note "unified-entry-runtime is the daemon-local substrate for F-intent-alignment-plan-execution-loop; mission_request is the user-facing review-packet/respond adapter, while unified_entry.rs is now a thin staged-runtime facade. The V3 physical split is explicit: stages.rs owns FLOW_REF plus s1_message_intake, s3_alignment_review_gate, s4_plan_authoring, s5_plan_review_gate, and s6_execution_runner; planner.rs owns plan_pipeline plus the pure directive/plan/execute argument builders; decorator.rs owns ArtifactScope, build_artifact_refs, decorate, and planner-error envelope projection; unified_entry/tests.rs owns the canonical loop, artifact-ref, pipeline-meta, and decorator regression pins so the facade stays small without losing behavior coverage. run_pipeline dispatches to run_directive_compile_stage, run_plan_compile_stage, and run_plan_execute_stage, then decorate stamps..."
      :evidence-sidecar ".missiond/v3/evidence/blueprint-notes.lisp#note-002")

    (surface file-artifacts
      :status "code-aligned"
      :implements [file-artifacts request-local-artifacts compat-artifact-paths]
      :code ["crates/missiond-daemon/src/handlers/knowledge/file_artifacts.rs"
             "crates/missiond-daemon/src/handlers/knowledge/file_artifacts/attempt.rs"
             "crates/missiond-daemon/src/handlers/knowledge/file_artifacts/kind.rs"
             "crates/missiond-daemon/src/handlers/knowledge/file_artifacts/write.rs"
             "crates/missiond-daemon/src/handlers/knowledge/file_artifacts/tests.rs"
             "scripts/check-v3-file-artifacts-isomorphism.mjs"]
      :note "file-artifacts is the shared writer layer for file-first Lisp artifacts. V3 physical split: file_artifacts.rs is the thin facade; kind.rs owns ArtifactKind, ArtifactSpec, artifact_path, sanitize_topic_segment, and stable compat path roots .missiond/alignment, .missiond/plans, and .missiond/workflows; write.rs owns unique_temp_path_in_dir, atomic_write_artifact, read_existing_metadata, and the temp-file + fsync + rename discipline; attempt.rs owns WriterContext, AttemptOutcome::Written, ResolveFailed, WriteFailed, resolve_writer_project_root, and attempt_artifact_write. mission_request and task-runner surfaces layer request-local artifact projection under .missiond/requests/<request_id>/ on top. The invariant is no partial writes: failed writes must not leak partial bytes, and callers must surface write_failed / partial status rather than pretending the Lisp artifact is authoritative. file_artifacts/tests.rs holds the writer regression suite outside the runtime facade.")

    (surface mission_directive
      :status "code-aligned"
      :implements [intent-alignment alignment-review-gate]
      :code ["crates/missiond-daemon/src/handlers/knowledge/directive.rs"
             "crates/missiond-daemon/src/handlers/knowledge/directive/compile_authoring.rs"
             "crates/missiond-daemon/src/handlers/knowledge/directive/approval_review.rs"
             "crates/missiond-daemon/src/handlers/knowledge/directive/approval_review/approve.rs"
             "crates/missiond-daemon/src/handlers/knowledge/directive/approval_review/archive.rs"
             "crates/missiond-daemon/src/handlers/knowledge/directive/approval_review/proposer.rs"
             "crates/missiond-daemon/src/handlers/knowledge/directive/approval_review/subscriber.rs"
             "crates/missiond-daemon/src/handlers/knowledge/directive/tests.rs"
             "crates/missiond-mcp/src/tools/knowledge/directive.rs"]
      :directive-review-boundary "directive/approval_review.rs owns the directive review facade; directive/approval_review/approve.rs owns directive approve transitions; directive/approval_review/archive.rs owns directive archive transitions."
      :model-projection "mission_directive sonnet compiler_model labels for intent-alignment authoring and directive-review proposals project from router-runtime-policy queued_sonnet_model through RouterRuntimeConfig; local Rust model literals are forbidden on these production paths."
      :note "directive.rs remains the mission_directive action facade plus list/get/version_chain store reads. directive/compile_authoring.rs owns intent-alignment authoring: dry_run emits a deterministic directive-draft Lisp artifact with utterance/source/status; sonnet output is accepted only when it is one balanced Lisp s-expression with head directive|directive-draft|intent-alignment. Persisted directive Lisp is enriched with :directive_id + :version before being surfaced as compiled_sexp(_preview) and before optional file-first writes. The compatibility file writer targets ArtifactKind::IntentAlignment at .missiond/alignment/<topic>/intent-alignment.lisp, never rolls back a committed row on file failure, and review_gate_policy only emits/records gates; it never auto-approves intent. directive/approval_review.rs owns approve/archive/review-resolution transitions, deterministic..."
      :evidence-sidecar ".missiond/v3/evidence/blueprint-notes.lisp#note-003")

    (surface mission_plan
      :status "code-aligned"
      :implements [plan plan-review-gate plan-runner evidence-collector]
      :code ["crates/missiond-daemon/src/handlers/knowledge/plan.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan/compile_authoring.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan/compile_authoring/artifact.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan/compile_authoring/validation.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan/approval_review.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan/approval_review/approve.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan/approval_review/mark.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan/approval_review/proposer.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan/approval_review/subscriber.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan/approval_review/supersede.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan/field_inference.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan/field_inference/mode.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan/field_inference/evidence.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan/field_inference/rules.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan/field_inference/llm.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan/field_inference/apply.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan/field_inference/apply/persisted.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan/execution_runtime.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan/execution_runtime/bridge.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan/execution_runtime/internal.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan/execution_runtime/workstation.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan/internal_dispatch.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan/execute_hints.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan/task_contract.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan/distill_chain.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan/dispatch_response.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan/evidence_sidecar.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan/router_policy_dry_run.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan/router_policy_dry_run/predicate.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan/router_policy_dry_run/readiness.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan/router_policy_dry_run/descriptor.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan/router_policy_dry_run/schema_parser.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan/task_runner_dry_run.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan/task_runner_dry_run/manifest.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan/task_runner_dry_run/projection.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan/tests.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan_dag.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan_dag/runtime.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan_dag/runtime/acceptance.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan_dag/runtime/bookkeeping.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan_dag/runtime/claiming.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan_dag/runtime/claims.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan_dag/runtime/drain.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan_dag/runtime/failures.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan_dag/runtime/gates.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan_dag/runtime/rollbacks.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan_dag/runtime/retry.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan_dag/runtime/skips.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan_dag/runtime/spawn.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan_dag/runtime/success.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan_dag/parser.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan_dag/parser/types.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan_dag/parser/types/node.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan_dag/parser/types/errors.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan_dag/parser/scanner.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan_dag/parser/scanner/top_level.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan_dag/parser/scanner/node_form.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan_dag/parser/scanner/lists.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan_dag/parser/scanner/keyword_pairs.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan_dag/parser/validation.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan_dag/acceptance.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan_dag/acceptance/types.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan_dag/acceptance/evaluator.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan_dag/acceptance/fan_in.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan_dag/acceptance/payload.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan_dag/acceptance/pause.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan_dag/claim_lease.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan_dag/dispatch.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan_dag/dispatch/types.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan_dag/dispatch/workstation.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan_dag/dispatch/task_contract_ctx.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan_dag/dispatch/runner.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan_dag/rollback.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan_dag/rollback/descriptor.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan_dag/rollback/run.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan_dag/rollback/types.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan_dag/rollback/types/node_ext.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan_dag/rollback/types/policy.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan_dag/rollback/types/evaluation.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan_dag/rollback/types/cascade.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan_dag/rollback/cascade.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan_dag/rollback/cascade/ordering.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan_dag/rollback/cascade/plan_entry.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan_dag/rollback/cascade/runner.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan_dag/rollback/cascade/dispatch_outcome.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan_dag/resume.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan_dag/resume/validation.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan_dag/resume/action.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan_dag/resume/evidence.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan_dag/resume/listener.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan_dag/outcome.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan_dag/outcome/state.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan_dag/outcome/node_result.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan_dag/outcome/execution.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan_dag/projection.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan_dag/finalization.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan_dag/lifecycle.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan_dag/lifecycle/context.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan_dag/lifecycle/event_ref.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan_dag/lifecycle/finalize.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan_dag/lifecycle/nodes.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan_dag/lifecycle/nodes/running.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan_dag/lifecycle/nodes/finished.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan_dag/lifecycle/nodes/rollback.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan_dag/lifecycle/nodes/acceptance.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan_dag/lifecycle/nodes/skipped.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan_dag/lifecycle/retry.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan_dag/lifecycle/review.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan_dag/lifecycle/claims.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan_dag/scheduler.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan_dag/mode.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan_dag/tests.rs"
             "crates/missiond-mcp/src/tools/knowledge/plan.rs"]
      :runtime-bookkeeping "plan_dag/runtime/bookkeeping.rs owns DAG runtime bookkeeping: node map, successor map, topo index, ready-id selection, running/pending scans, and topological outcome stitching."
      :runtime-acceptance "plan_dag/runtime/acceptance.rs owns DAG runtime success acceptance projection: per-node acceptance evaluation, fan-in overlay, acceptance evidence emission, terminal lifecycle/state selection, manual pause id projection, and success-branch acceptance result packaging."
      :runtime-claiming "plan_dag/runtime/claiming.rs owns DAG runtime dispatch claim preparation: initial claim acquisition, strict claim-conflict refusal, compat conflict audit projection, conflict payload construction, taint propagation, and fail-fast claim-conflict signaling."
      :runtime-claims "plan_dag/runtime/claims.rs owns DAG runtime claim acquisition and release projection: acquired/compat claim evidence, claimed lifecycle projection, active claim map updates, recorded claim lookup, lease release, terminal-state label threading, and compatibility no-op release for unrecorded claims."
      :runtime-drain "plan_dag/runtime/drain.rs owns DAG runtime wave drain projection: JoinSet result unwrapping, finish evidence emission, successful dispatch handoff, retry handoff, terminal failure handoff, local fail-fast abort tracking, and scheduler error egress."
      :runtime-failures "plan_dag/runtime/failures.rs owns DAG runtime final failure projection: terminal failed lifecycle, claim release, rollback evaluation, failed NodeResult projection, downstream taint propagation, and fail-fast abort signaling."
      :runtime-gates "plan_dag/runtime/gates.rs owns DAG runtime ready-node gate filtering: condition-gated skips, review-gate pause projection, ready dispatch cap, and gate-local taint propagation."
      :runtime-rollbacks "plan_dag/runtime/rollbacks.rs owns DAG runtime rollback evaluation: node-local rollback, cascade rollback fold-in, inactive rollback suppression, rollback evidence emission, and RollbackEvaluation projection for terminal node results."
      :runtime-retry "plan_dag/runtime/retry.rs owns DAG runtime retry projection: retry predicate application, failed-attempt claim release, optional retry backoff, retry attempt bumping, retry claim reacquisition or compat conflict recording, and same-wave dispatch respawn."
      :runtime-skips "plan_dag/runtime/skips.rs owns DAG runtime skip materialization: tainted pending skips, fail-fast pending force-skips, skip evidence emission, and skipped NodeResult projection."
      :runtime-spawn "plan_dag/runtime/spawn.rs owns DAG runtime dispatch spawn projection: running lifecycle transition, running evidence emission, task-contract context clone, AppState/Plan clone, and JoinSet dispatch task spawn."
      :runtime-success "plan_dag/runtime/success.rs owns DAG runtime successful dispatch projection: success acceptance handoff, terminal claim release, acceptance-rejected rollback, accepted NodeResult projection, rejection taint propagation, and fail-fast rejection signaling."
      :model-projection "mission_plan sonnet compiler_model labels for plan-authoring, plan-review proposals, and field-inference proposals project from router-runtime-policy queued_sonnet_model through RouterRuntimeConfig; local Rust model literals are forbidden on these production paths."
      :note "compiler_mode=dry_run now renders plan-draft as an executable Lisp scaffold with :target, :objective, and :nodes; execute can derive target_source=plan_hint from plan.sexp_text instead of caller escape parameters."
      :evidence-sidecar ".missiond/v3/evidence/blueprint-notes.lisp#note-004")

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

    (surface typed-lisp-compiler
      :status "code-aligned"
      :implements [lisp-reader typed-ast semantic-validator diagnostic-json projection-json semantic-ir-json structured-runtime-config-json structured-project-universe-json structured-workflow-contract-json workflow-directory-structural-gate project-directory-structural-gate workstation-config-structural-gate project-m6-depth-gate runtime-compiled-json-loader auth-domain-sample]
      :code ["tools/missiond_lispc/dune-project"
             "tools/missiond_lispc/bin/dune"
             "tools/missiond_lispc/bin/main.ml"
             "tools/missiond_lispc/bin/ast.ml"
             "tools/missiond_lispc/bin/parser.ml"
             "tools/missiond_lispc/bin/schema_v3.ml"
             "tools/missiond_lispc/bin/workflow_schema.ml"
             "tools/missiond_lispc/bin/project_schema.ml"
             "tools/missiond_lispc/bin/workstation_schema.ml"
             "tools/missiond_lispc/bin/emit_json.ml"
             "tools/missiond_lispc/test/dune"
             "tools/missiond_lispc/test/parser_golden.ml"
             "scripts/lib/ocaml_lispc.mjs"
             "scripts/lib/v3_compiled_contract.mjs"
             "scripts/check-ocaml-toolchain.mjs"
             "scripts/check-typed-lisp-compiler.mjs"
             "scripts/compile-v3-runtime.mjs"
             "scripts/check-auth-domain-ssot.mjs"
             "scripts/check-project-domain-hardening.mjs"
             "crates/missiond-daemon/src/context/v3_blueprint_runtime.rs"
             ".missiond/workflows/typed-lisp-compiler-convergence.lisp"]
      :note "Lisp remains the canonical authoring SSOT. The OCaml layer is a dev-time typed compiler/checker/projection layer for source-located diagnostics and generated runtime JSON; compiled-runtime-config.json carries workstation/flow/compute/router/autopilot/learning runtime policy, project universe and workflow projections include structured project/maturity/workflow payloads, workflow-directory gates validate every .missiond/workflows/*.lisp contract, project-directory structural gates validate each registered project's active blueprint shards before M5 maturity can rely on project-local shape evidence, and M6-depth gates validate Auth-grade domain/policy/flow/event/runtime/compatibility evidence. OCaml is not in the daemon hot path. JS checkers remain compatibility wrappers and code-anchor validators, but their live surface/function facts are loaded through scripts/lib/v3_compiled_contract.mjs from missiond-lispc emit-v3 / emit-semantic-ir instead of a hand-maintained surface list.")

    (surface semantic-ir-compiler
      :status "code-aligned"
      :implements [semantic-ir-json compact-agent-slices source-map-diagnostics compiled-workflow-contracts]
      :code ["tools/missiond_lispc/bin/emit_json.ml"
             "tools/missiond_lispc/bin/main.ml"
             "scripts/compile-v3-runtime.mjs"
             "scripts/check-v3-shared-memory-isomorphism.mjs"]
      :note "The semantic IR compiler is the compact projection layer between human/agent Lisp SSOT and worker context slices. It emits typed facts with short ids and source maps into compiled-semantic-ir.json, derives compiled-agent-slices.json for agents, and keeps compiled-workflow-contracts.json aligned with workflow Lisp. Generated JSON is machine-oriented and never hand-authored.")

    (surface mission-shared-memory
      :status "code-aligned"
      :implements [shared-events shared-artifacts shared-claims agent-cursors context-slices evidence-governance-view task-delegate-write-lease swarm-write-lease]
      :code ["crates/missiond-core/migrations/20260508000000_shared_memory.sql"
             "crates/missiond-daemon/src/engine/shared_memory.rs"
             "crates/missiond-daemon/src/state.rs"
             "crates/missiond-daemon/src/main.rs"
             "crates/missiond-daemon/src/engine/master_control.rs"
             "crates/missiond-daemon/src/handlers/knowledge/shared_memory.rs"
             "crates/missiond-daemon/src/handlers/mod.rs"
             "crates/missiond-daemon/src/handlers/compute/task_delegate.rs"
             "crates/missiond-mcp/src/tools/knowledge/shared_memory.rs"
             "crates/missiond-mcp/src/tools/mod.rs"
             "scripts/check-v3-shared-memory-isomorphism.mjs"
             ".missiond/workflows/semantic-ir-shared-memory-convergence.lisp"]
      :note "MissionD shared memory is the Rust/Postgres durable coordination substrate for concurrent agents. EventBus wakes and observes; shared_events/shared_artifacts/shared_claims/agent_cursors hold the coordination truth. Investigation workers write artifacts without claims; implementation workers must have an accepted shard and write-scope lease. mission_shared_memory(action=evidence_view) projects the unified Memory/KB + Logs/Timeline/Conversation model: task_result_artifacts are canonical worker outputs, conversations are provider/user turn read models, event_log/shared_events are causality, KB is reviewed long-term knowledge, and BoardTask state is coordination projection. Legacy shared-memory.lisp ledgers remain compatibility projections, not concurrent write authority.")

    (surface evidence-governance-view
      :status "code-aligned"
      :implements [evidence-governance-view task-result-artifact-authority conversation-read-model timeline-causality-view reviewed-kb-memory board-coordination-projection]
      :code ["crates/missiond-daemon/src/engine/shared_memory.rs"
             "crates/missiond-mcp/src/tools/knowledge/shared_memory.rs"
             "scripts/check-v3-shared-memory-isomorphism.mjs"
             ".missiond/v3/missiond-blueprint.lisp"]
      :acceptance ["node scripts/check-v3-shared-memory-isomorphism.mjs --json"
                   "node scripts/check-v3-pillar-flow-schema.mjs --engine=ocaml --json"]
      :note "evidence-governance-view is the unified read surface that prevents Memory/KB, Logs, Timeline, Conversation, and Board from competing as result authorities. It is served through mission_shared_memory(action=evidence_view), but it remains a distinct SSOT surface so pillar-flow keeps a single function-to-surface mapping.")

    (surface review-gate
      :status "code-aligned"
      :implements [alignment-review-gate plan-review-gate workflow-review-gate two-gate-default]
      :code ["crates/missiond-daemon/src/handlers/knowledge/review_gate.rs"
             "crates/missiond-daemon/src/handlers/knowledge/review_gate/created.rs"
             "crates/missiond-daemon/src/handlers/knowledge/review_gate/resolution.rs"
             "crates/missiond-daemon/src/handlers/knowledge/review_gate/resolution/automation.rs"
             "crates/missiond-daemon/src/handlers/knowledge/review_gate/resolution/emitter.rs"
             "crates/missiond-daemon/src/handlers/knowledge/review_gate/resolution/envelope.rs"
             "crates/missiond-daemon/src/handlers/knowledge/review_gate/resolution/input.rs"
             "crates/missiond-daemon/src/handlers/knowledge/review_gate/resolution/payload.rs"
             "crates/missiond-daemon/src/handlers/knowledge/review_gate/resolution/subscriber.rs"
             "crates/missiond-daemon/src/handlers/knowledge/review_gate/auto_answer.rs"
             "crates/missiond-daemon/src/handlers/knowledge/review_gate/llm_approval.rs"
             "crates/missiond-daemon/src/handlers/knowledge/review_gate/llm_approval/proposal.rs"
             "crates/missiond-daemon/src/handlers/knowledge/review_gate/llm_approval/apply_gate.rs"
             "crates/missiond-daemon/src/handlers/knowledge/review_gate/llm_approval/apply_gate/evaluate.rs"
             "crates/missiond-daemon/src/handlers/knowledge/review_gate/llm_approval/apply_gate/hash.rs"
             "crates/missiond-daemon/src/handlers/knowledge/review_gate/llm_approval/apply_gate/input.rs"
             "crates/missiond-daemon/src/handlers/knowledge/review_gate/llm_approval/apply_gate/outcome.rs"
             "crates/missiond-daemon/src/handlers/knowledge/review_gate/llm_approval/apply_gate/payload.rs"
             "crates/missiond-daemon/src/handlers/knowledge/review_gate/llm_approval/apply_gate/preflight.rs"
             "crates/missiond-daemon/src/handlers/knowledge/review_gate/tests.rs"
             "crates/missiond-daemon/src/handlers/knowledge/directive.rs"
             "crates/missiond-daemon/src/handlers/knowledge/directive/approval_review.rs"
             "crates/missiond-daemon/src/handlers/knowledge/directive/approval_review/approve.rs"
             "crates/missiond-daemon/src/handlers/knowledge/directive/approval_review/archive.rs"
             "crates/missiond-daemon/src/handlers/knowledge/directive/approval_review/proposer.rs"
             "crates/missiond-daemon/src/handlers/knowledge/directive/approval_review/subscriber.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan/compile_authoring.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan/approval_review.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan/approval_review/approve.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan/approval_review/mark.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan/approval_review/proposer.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan/approval_review/subscriber.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan/approval_review/supersede.rs"
             "crates/missiond-daemon/src/handlers/knowledge/workflow.rs"
             "crates/missiond-daemon/src/handlers/knowledge/workflow/compile_methodology.rs"
             "crates/missiond-mcp/src/tools/knowledge/directive.rs"
             "crates/missiond-mcp/src/tools/knowledge/plan.rs"
             "crates/missiond-mcp/src/tools/knowledge/workflow.rs"
             "scripts/check-v3-review-gate-isomorphism.mjs"]
      :directive-review-boundary "directive/approval_review.rs owns directive review facade wiring; directive/approval_review/approve.rs owns directive approve resolution/policy/apply-gate transitions; directive/approval_review/archive.rs owns destructive archive resolution/policy/apply-gate refusal."
      :note "review-gate is the shared event-bus review layer behind alignment-review-gate, plan-review-gate, workflow review, and the V3 two-gate-default axiom; it must never auto-approve without explicit caller approval plus matching proposal hash."
      :evidence-sidecar ".missiond/v3/evidence/blueprint-notes.lisp#note-009")

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

    (surface source-hygiene
      :status "code-aligned"
      :implements [source-hygiene scoped-write-gate ssot-retrieval-scope]
      :code ["scripts/check-staged-source-hygiene.mjs"
             "scripts/task-scope-guard.mjs"
             "scripts/check-missiond-hooks.mjs"
             "scripts/install-missiond-hooks.mjs"
             ".githooks/pre-commit"
             "scripts/verify-task-runner-batch.mjs"
             "scripts/check-v3-source-hygiene-isomorphism.mjs"
             "scripts/check-v3-runtime-path-hygiene.mjs"]
      :note "check-staged-source-hygiene.mjs is the read-only staged/source preflight: default mode reads staged ACMR files, rejects raw NUL bytes from staged blobs, runs git diff --cached --check, and delegates to task-scope-guard.mjs when --task or MISSIOND_TASK_CONTRACT is set; --files mode checks supplied files without reading git blobs. task-scope-guard.mjs owns task contract write-scope/must-not-touch enforcement for staged and commit modes. .githooks/pre-commit runs task-contract hygiene when MISSIOND_TASK_CONTRACT is set, then always runs missiond-work-order verify --staged so code-like staged files require MissionD-Work-Order coverage; check-missiond-hooks.mjs is a read-only doctor and install-missiond-hooks.mjs is the only mutating hook installer. verify-task-runner-batch imports checkSuppliedFiles for source-hygiene fixture coverage without mutating git. ssot-retrieval-scope keeps broad review/search on active authoring Lisp and treats .missiond/v3/runtime/** reports as cold diagnostic evidence unless include_runtime=true or a concrete trace path is requested.")

    (surface lisp-code-drift-policy
      :status "code-aligned"
      :implements [lisp-code-drift]
      :code [".missiond/v3/missiond-blueprint.lisp"
             "crates/missiond-daemon/src/engine/master_control.rs"
             "crates/missiond-daemon/src/handlers/knowledge/board/update.rs"
             "scripts/check-v3-direct-code-drift-policy.mjs"
             "scripts/check-v3-code-isomorphism-complete.mjs"]
      :note "lisp-code-drift-policy is the governance surface for code-first exceptions. Normal behavior changes must carry a same-task Lisp/checker delta or map to an already pinned surface. Emergency code-first fixes are allowed only with waiver metadata and must immediately create a visible backfill BoardTask that adds the missing blueprint, checker, and evidence. The runtime close gate in mission_board_update/mission_board_batch_update/mission_board_toggle blocks status=done while unresolved code-first drift exists, so code-first work cannot be closed without Lisp/checker/evidence convergence.")

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
      :note "lisp-code-sync-loop is the event-driven Lisp->code isomorphism muscle. It watches active ProjectRegistry .missiond authoring paths, emits SystemEvent::ConfigChanged, ignores .missiond/v3/runtime/** and cold evidence, suppresses unchanged content fingerprints before EventBus publication and again when consuming queued ConfigChanged events, debounces, runs typed compile plus code-isomorphism gates, writes bounded reports, exposes reportDirs/stormCircuitHits/recentSyncTaskCreations, creates one deduped BoardTask for failing gates, switches to lisp-code-sync:<project>:storm-circuit on same-source storms, and lets Autopilot close stale runtime-report BoardTasks as resolved_by_runtime_fix/stale_evidence before slot selection. It never edits code directly; mutation still requires evidence-plan, accepted exact shard, write_scope, acceptance, and durable green gates.")

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

    (surface context-pack
      :status "code-aligned"
      :implements [multi-agent-context-pack]
      :code ["scripts/check-context-pack.mjs"
             "scripts/context-pack-append.mjs"
             "scripts/context-pack-compile-shards.mjs"
             "scripts/context-pack-materialize-wave.mjs"
             "scripts/context-pack-run-wave.mjs"
             "scripts/lib/v3_workstation_runtime.mjs"
             "crates/missiond-daemon/src/handlers/compute/task_delegate.rs"
             "scripts/check-v3-context-pack-isomorphism.mjs"]
      :note "Context-pack is the V3 high-density planning surface for two-stage parallel work: context investigators append claim/observation/anchor/shard-proposal/conflict entries to .missiond/tasks/<wave>/context-pack.lisp without code edits, then an orchestrator/integrator appends integration-plan with accepted-shards and dispatch-groups. Mapped dispatch groups use (group :id <id> :shards [...]) so scripts/context-pack-compile-shards.mjs can project the Lisp plan into dispatchable_groups for code workers; legacy bare group ids remain names_only for older packs. mission_swarm_run materializes a lightweight missiond.swarm-context-pack.v1 sidecar before publishing worker BoardTasks so provider workers can read the declared context_pack_path."
      :evidence-sidecar ".missiond/v3/evidence/blueprint-notes.lisp#note-011")

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
	    :note "mission_compute_slot and mission_task_delegate accept model/model_profile; coder/researcher default to Claude Code Default(Opus 4.7/1M) by omitting --model. mission_task_delegate also accepts two-stage delegation metadata (task_class, pool_hint, engine_hint, context_pack_path, read_scope, write_scope, must_not_touch, acceptance) and records it into the BoardTask description for Autopilot worker prompts; the scope_semantics contract separates readable evidence from writable scope and prevents must_not_touch from being misread as a read ban. main.rs startup SlotManager registration loads WorkstationRuntimeConfig and generates persistent SlotTaskConfig rows by iterating workstation-config startup-slot entries; ClaudeCode startup slots project their model_profile through spawn_model_for_profile, so arch maintenance and Lisp survey no longer hardcode claude-sonnet-4-6 or local timeout literals."
	    :evidence-sidecar ".missiond/v3/evidence/blueprint-notes.lisp#note-012")

    (surface workstation-pool
      :status "code-aligned"
      :implements [workstation-pool]
      :code ["crates/missiond-daemon/src/context/v3_blueprint_runtime.rs"
             "crates/missiond-daemon/src/main.rs"
             "crates/missiond-pty/src/session.rs"
             "crates/missiond-pty/src/manager.rs"
             "crates/missiond-core/src/types/slot.rs"
             "crates/missiond-daemon/src/engine/intent_engine/autopilot.rs"
             "crates/missiond-daemon/src/handlers/compute/compute_slot.rs"
             "crates/missiond-daemon/src/handlers/compute/slot.rs"
             "crates/missiond-core/src/core/slot_manager.rs"
             "scripts/check-v3-workstation-pool-isomorphism.mjs"]
      :evidence ".missiond/v3/evidence/workstation-pool.lisp"
      :note "workstation-pool is the compact V3 compute-account SSOT. It declares ClaudeCode Opus/Sonnet lanes, Gemini read-only lanes, and the non-shard Codex master lane; runtime projection feeds SlotManager, PTYSpawnOptions, Autopilot routing, mission_compute_slot list, and mission_slots legacy-Sonnet filtering. mission_slots MUST project activeBoardTaskId/currentTaskId and activeBoardTask by joining running BoardTasks on assignee or pty_slot claim so the Board cockpit can show what each visible PTY is actually doing."
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
      :note "autopilot-runtime is the event-driven muscle layer for delegated BoardTasks. task_delegate and mission_board_create publish BoardEvent::TaskCreated; v2_subscribers owns the event-bus nerves v2_autopilot_board_event and v2_autopilot_slot_event, which wake board_dispatch_notify on BoardEvent::TaskCreated, reopened BoardEvent status updates, and SlotEvent::BecameIdle, then ack immediately without running pty.send inline. The dedicated Autopilot task remains the only prompt/close owner: it claims eligible open BoardTasks, derives leases/timeouts from V3 policy, holds a per-slot dispatch guard across state.pty.send, emits SlotEvent::TaskDispatched, synthesizes mission_execution completion when needed, and closes/preserves the BoardTask according to execution-ownership delegated-boardtask. This preserves the event-bus causal chain while keeping long-running worker interaction outside subscriber ack paths.")

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
      :note "genome-runtime introduces MissionD's Lisp Genome -> Rust Atom/Cell/Tissue/Organ runtime boundary. missiond-lispc validates genome Lisp and emits compiled-genomes JSON; missiond-kernel owns EventEnvelope, Effect, CommandEnvelope, AtomRegistry, Molecule, RuleGraph, Cell, TissueProfile, Genome, and ActivationMode; missiond-organism-runtime executes Cell::on_event under shadow, active, or rollback activation with budget/idempotency guards. The first migrated organ is Autopilot: board/slot subscribers run shadow parity against legacy wakeup helpers by default, active mode routes notifications/ticks/dispatch through AutopilotEffectInterpreter, and runtime errors publish incidents while falling back to the legacy path.")

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
      :note "mission_board is the durable BoardTask coordination surface underneath delegated ClaudeCode work: MCP exposes query/create/update/delete/claim/decompose/retry/note_add with a generated schema from .missiond/intent-tools.lisp. Board handlers normalize common snake_case/camelCase aliases before schema projection, reject invalid status/noteType with structured ToolError codes, validate parentId/dependsOn before persistence, cap descriptions, reject oversized note payloads with artifact-path guidance, return compact note receipts for large stored content, and aggregate self-heal incident tasks by dedupe_key instead of auto-executing a worker per tool outage so agents recover instead of flailing on unknown errors."
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

    (surface memory-kb
      :status "code-aligned"
      :implements [knowledge-memory kb-manager memory insight intent-snapshot]
      :code ["crates/missiond-daemon/src/context/v3_blueprint_runtime.rs"
             "crates/missiond-daemon/src/handlers/knowledge/kb.rs"
             "crates/missiond-daemon/src/handlers/knowledge/kb/args.rs"
             "crates/missiond-daemon/src/handlers/knowledge/kb/remember.rs"
             "crates/missiond-daemon/src/handlers/knowledge/kb/quality.rs"
             "crates/missiond-daemon/src/handlers/knowledge/kb/compact.rs"
             "crates/missiond-daemon/src/handlers/knowledge/kb/conflicts.rs"
             "crates/missiond-daemon/src/handlers/knowledge/kb/query.rs"
             "crates/missiond-daemon/src/handlers/knowledge/kb/discovery.rs"
             "crates/missiond-daemon/src/handlers/knowledge/kb/analyze.rs"
             "crates/missiond-daemon/src/handlers/knowledge/kb/mutate.rs"
             "crates/missiond-daemon/src/handlers/knowledge/kb/import.rs"
             "crates/missiond-daemon/src/handlers/knowledge/kb/gc.rs"
             "crates/missiond-daemon/src/handlers/knowledge/kb/ops.rs"
             "crates/missiond-daemon/src/handlers/knowledge/kb/beacon.rs"
             "crates/missiond-daemon/src/handlers/knowledge/kb/code_search.rs"
             "crates/missiond-daemon/src/handlers/knowledge/kb/review.rs"
             "crates/missiond-daemon/src/handlers/knowledge/context_gather.rs"
             "crates/missiond-mcp/src/tools/knowledge/context_gather.rs"
             "crates/missiond-core/migrations/20260508001000_knowledge_review_state.sql"
             "crates/missiond-core/src/types/knowledge.rs"
             "crates/missiond-core/src/db/traits.rs"
             "crates/missiond-core/src/db/pg/knowledge.rs"
             "crates/missiond-daemon/src/handlers/knowledge/memory.rs"
             "crates/missiond-daemon/src/engine/learning_engine/mod.rs"
             "crates/missiond-daemon/src/engine/learning_engine/extraction.rs"
             "crates/missiond-daemon/src/engine/learning_engine/decision_engine.rs"
             "crates/missiond-daemon/src/engine/learning_engine/timeline_analyst.rs"
             "crates/missiond-daemon/src/engine/learning_engine/idle_explorer.rs"
             "crates/missiond-daemon/src/engine/learning_engine/historical_scanner.rs"
             "crates/missiond-core/src/db/pg/conversation.rs"
             "crates/missiond-daemon/src/handlers/knowledge/insight.rs"
             "crates/missiond-daemon/src/handlers/knowledge/intent.rs"
             "crates/missiond-mcp/src/tools/knowledge/kb.rs"
             "crates/missiond-mcp/src/tools/knowledge/memory.rs"
             "crates/missiond-mcp/src/tools/knowledge/insight.rs"
             "crates/missiond-mcp/src/tools/knowledge/intent.rs"
             "scripts/check-v3-memory-kb-isomorphism.mjs"]
	      :note "Runtime-projected V3 destination for memory/KB tools. memory-kb-policy and learning-engine-policy own budgets, cadences, bounded SQL probes, shared KB dedupe gate semantics, and physical split ownership across kb/* modules. KbStore::kb_remember is the shared write gate for realtime/deep-analysis/manual/internal memory writes; same-session duplicates preserve evidence_refs/source_sessions/superseded_by provenance instead of creating two active keys. kb/review.rs owns non-destructive knowledge_review_state overlay so stale memories leave default retrieval without deleting evidence; low-confidence semantic duplicates become needs-human review artifacts. Conversation history distillation remains deferred behind .missiond/workflows/conversation-memory-distillation.lisp."
	      :evidence-sidecar ".missiond/v3/evidence/blueprint-notes.lisp#note-015")

    (surface codex-boot-context
      :status "code-aligned"
      :implements [codex-boot-context mission_context_boot boot-capsule external-chat-handoff]
      :code [".missiond/v3/evidence/codex-boot-context.lisp"
             "crates/missiond-daemon/src/handlers/knowledge/context_gather.rs"
             "crates/missiond-daemon/src/handlers/mod.rs"
             "crates/missiond-mcp/src/tools/knowledge/context_gather.rs"
             "scripts/check-v3-codex-boot-context-isomorphism.mjs"]
      :acceptance ["node scripts/check-v3-codex-boot-context-isomorphism.mjs --json"]
      :note "codex-boot-context is the small versioned startup capsule for resident Codex, Codex workers, and external Codex handoffs. It carries only collaboration protocol and layer rules; task/project facts must be gathered through mission_context_gather and cold evidence stays explicit. This lets new conversations start with MissionD's learned operating contract without injecting raw chat history, secrets, provider logs, or unreviewed KB.")

    (surface project-registry
      :status "code-aligned"
      :implements [project-registry project-root-resolution service-runtime-universe]
      :code ["crates/missiond-daemon/src/context/v3_blueprint_runtime.rs"
             "crates/missiond-daemon/src/handlers/knowledge/project.rs"
             "crates/missiond-daemon/src/handlers/knowledge/project/registry.rs"
             "crates/missiond-daemon/src/handlers/knowledge/project/context.rs"
             "crates/missiond-daemon/src/handlers/knowledge/project/universe.rs"
             "crates/missiond-daemon/src/handlers/knowledge/project/reconcile.rs"
             "crates/missiond-daemon/src/handlers/knowledge/project/survey.rs"
             "crates/missiond-daemon/src/handlers/knowledge/project/vault.rs"
             "crates/missiond-core/src/types/project.rs"
             "crates/missiond-daemon/src/slot_orchestrator/project_root.rs"
             "crates/missiond-mcp/src/tools/knowledge/project.rs"
             "scripts/check-v3-project-registry-isomorphism.mjs"
             "scripts/check-project-maturity.mjs"]
      :note "Code-aligned destination for project registry/root resolution. project.rs is the mission_project facade; project/registry.rs owns list/get/set_active/sync/init/import_universe; project/universe.rs owns mission_project(action=universe) and projects service-runtime-universe entries such as auth production domain/deployment/DNS capability to master, workers, and Board System. ProjectRegistry::resolve owns longest-prefix project lookup; inactive project aliases never participate in cwd resolution, and mission_project init archives inactive path aliases before upsert so stale aliases cannot block canonical root correction. check-project-maturity.mjs is the project-universe maturity gate: --min-level M5 proves worker-operational SSOT closure; --min-level M6 proves Auth-grade domain/policy/flow/event/runtime/compatibility depth. It resolves the MissionD blueprint from the checker script directory so external-project workers can run it from the target root."
      :evidence-sidecar ".missiond/v3/evidence/blueprint-notes.lisp#note-016")

    (surface data-residency-universe
      :status "code-aligned"
      :implements [data-residency-universe data-region-partition-contract xjp-platform-partition-contract cross-region-data-policy project-region-declaration]
      :code [".missiond/v3/missiond-blueprint.lisp"
             ".missiond/research/data-residency-universe-report-20260512.md"
             "scripts/check-v3-data-residency-universe-isomorphism.mjs"
             "scripts/check-project-ssot-universe.mjs"
             "scripts/check-project-maturity.mjs"
             "/Users/jinchen/Downloads/PCEA develop/.missiond/intent.lisp"
             "/Users/jinchen/Downloads/PCEA develop/.missiond/check.sh"]
      :note "data-residency-universe is the SSOT surface for cn/global hard partitions and global-eu operating-zone policy. It now models XJP platform partitions first: xjp-cn owns the Aliyun ECS/CN infra stack, xjp-global owns the GCP/global stack, and app-level partitions such as pcea-cn/pcea-global or cuthub-cn/cuthub-global bind to those stacks. It keeps region identity, issuer, secret, storage, payment ledger, model router, event, deploy, and cross-region egress rules out of ad hoc deployment notes. PCEA's project-local .missiond/check.sh pins the same declarations so data-bearing M6 means platform-partition-aware, not merely code-mapped.")

    (surface eventbridge
      :status "code-aligned"
      :implements [eventbridge-policy deployment-event-ingest deploy-agent-self-update-governance]
      :code ["crates/missiond-core/src/ws/server.rs"
             "crates/missiond-core/src/event/events/system.rs"
             "crates/missiond-daemon/src/bus/bootstrap.rs"
             "crates/missiond-daemon/src/bus/v2_subscribers.rs"
             "crates/missiond-daemon/src/handlers/comm/timeline.rs"
             "crates/missiond-mcp/src/tools/comm/timeline.rs"
             ".missiond/workflows/deployment-event-response.lisp"
             "scripts/check-v3-eventbridge-isomorphism.mjs"]
      :note "MissionD local EventBridge accepts missiond.event-envelope.v1 cloud/service events, preserves project/correlation metadata under ExternalServiceEvent payload._envelope, dedupes by service/event id, and exposes EventBus waits for deployment workflows. deploy-center remains deployment fact authority; MissionD caches, displays, and triggers Board workflows only.")

    (surface memory-provider-boundary
      :status "code-aligned"
      :implements [memory-provider-contract memory-provider]
      :code [".missiond/research/missiond-memory-eventhub-modularization-20260512.md"
             ".missiond/workflows/missiond-module-extraction.lisp"
             ".missiond/v3/missiond-blueprint.lisp"
             "scripts/manage-local-providers.sh"
             "scripts/check-v3-service-extraction-isomorphism.mjs"
             "scripts/check-v3-code-isomorphism-complete.mjs"]
      :note "Pinned modularization decision: MissionD Core remains local orchestrator and long-term memory becomes a MemoryProvider contract with null/local/xjp provider implementations. mission_kb_* remains a compatibility facade; providers own conversation stores, active memory, review overlay, skill evidence, FTS, embedding, rerank, export, purge, and tenant/universe/project/user isolation.")

    (surface eventhub-service-boundary
      :status "code-aligned"
      :implements [eventhub-service-contract eventhub-service]
      :code [".missiond/research/missiond-memory-eventhub-modularization-20260512.md"
             ".missiond/workflows/missiond-module-extraction.lisp"
             ".missiond/v3/missiond-blueprint.lisp"
             "scripts/manage-local-providers.sh"
             "scripts/check-v3-service-extraction-isomorphism.mjs"
             "scripts/check-v3-code-isomorphism-complete.mjs"]
      :note "Pinned modularization decision: cross-service durable events move toward xjp-eventhub while MissionD keeps local EventBus and outbound spool for offline-safe agent orchestration. xjp-eventhub owns durable streams, cursors, subscriptions, waits, dead-letter, and replay for deploy-center/auth/router/timeline/service events.")

    (surface board-frontend
      :status "code-aligned"
      :implements [board-frontend]
      :code [".missiond/frontend/board-blueprint.lisp"
             ".missiond/frontend/evidence/board-blueprint-notes.lisp"
             ".missiond/frontend/evidence/board-frontend-convergence-report.lisp"
             "packages/board/src/generated/board-frontend-config.ts"
             "packages/board/src/App.tsx"
             "packages/board/src/types.ts"
             "packages/board/src/api.ts"
             "packages/board/src/store.ts"
             "packages/board/src/eventStream.ts"
             "packages/board/src/lib/missiond.ts"
             "packages/board/src/components/Terminal.tsx"
             "packages/board/src/components/TaskDialog.tsx"
             "packages/board/src/components/timeline/constants.tsx"
             "packages/board/src/components/timeline/helpers.ts"
             "packages/board/src/app/api/slots/route.ts"
             "scripts/project-frontend-board-config.mjs"
             "scripts/check-frontend-board-lisp-schema.mjs"
             "scripts/check-frontend-board-code-isomorphism.mjs"
             "scripts/check-frontend-board-runtime-projection.mjs"]
      :note "Board frontend is now a project-local Lisp SSOT registered from V3: .missiond/frontend/board-blueprint.lisp owns app-shell, MissionD proxy, BoardTask UI, workstation terminal, event stream, timeline/log, knowledge/system, and design-system pillars. The frontend checkers pin the same entry/core/egress/function structure as backend V3 while keeping the backend blueprint compact for the later 20+ project pattern. Runtime workstation/PTY identity must project through mission_slots + mission_pty_status; static frontend slot pools are forbidden.")

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

    (surface router-policy
      :status "code-aligned"
      :implements [router-policy-dry-run router-backend-readiness router-dispatch-descriptor]
      :code ["crates/missiond-mcp/src/tools/comm/router_chat.rs"
             "crates/missiond-daemon/src/handlers/comm/router_chat.rs"
             "crates/missiond-daemon/src/handlers/comm/router_chat/chat.rs"
             "crates/missiond-daemon/src/handlers/comm/router_chat/files.rs"
             "crates/missiond-daemon/src/handlers/comm/router_chat/manage.rs"
             "crates/missiond-daemon/src/context/v3_blueprint_runtime.rs"
             "crates/missiond-daemon/src/main.rs"
             "crates/missiond-daemon/src/workers/sonnet/embedding_worker.rs"
             "crates/missiond-daemon/src/llm/gemini_client.rs"
             "crates/missiond-daemon/src/llm/gemini_cli.rs"
             "crates/missiond-daemon/src/llm/gemini_driver.rs"
             "crates/missiond-daemon/src/llm/gemini_file_api.rs"
             "crates/missiond-daemon/src/llm/llm_gateway.rs"
             "crates/missiond-daemon/src/llm/sonnet_gateway.rs"
             "crates/missiond-daemon/src/llm/xjp_router_client.rs"
             "crates/missiond-daemon/src/infra/message_handler.rs"
             "crates/missiond-core/src/db/pg/observability.rs"
             "crates/missiond-core/src/ws/server.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan/router_policy_dry_run.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan/router_policy_dry_run/predicate.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan/router_policy_dry_run/readiness.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan/router_policy_dry_run/descriptor.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan/router_policy_dry_run/schema_parser.rs"
             "scripts/check-router-policy.mjs"
             "scripts/check-router-backend-registry.mjs"
             "scripts/check-router-dispatch-descriptor.mjs"
             "scripts/check-v3-router-policy-isomorphism.mjs"]
      :note "Runtime-projected V3 destination for the V2 router-policy dry-run chain and public router chat tools. router-runtime-policy owns chat/flow/Sonnet defaults, queue/tool/file timeouts, xjp-router embedding timeout, and token/compression budgets through RouterRuntimeConfig. router_chat/chat.rs owns mission_router_chat request normalization, LLM dispatch, persistence, and response projection; proxy mode carries the direct transcript prompt without sending /clear to shared PTY. Jarvis stream governance MUST write provider usage into token_usage_ledger through message_handler.rs and observability.rs so billing is a durable ledger. router_chat/files.rs owns attachment denylist; router_chat/manage.rs owns history/list/delete/clear/stats/compress."
      :evidence-sidecar ".missiond/v3/evidence/blueprint-notes.lisp#note-018")

    (surface incident-governance
      :status "code-aligned"
      :implements [question incident llm-trace decision-stats auth]
      :code ["crates/missiond-daemon/src/handlers/comm/question.rs"
             "crates/missiond-daemon/src/handlers/comm/question/question_flow.rs"
             "crates/missiond-daemon/src/handlers/comm/question/decision.rs"
             "crates/missiond-daemon/src/handlers/comm/question/llm_trace.rs"
             "crates/missiond-daemon/src/handlers/comm/question/auth.rs"
             "crates/missiond-daemon/src/handlers/comm/question/incident.rs"
             "crates/missiond-daemon/src/handlers/mod.rs"
             "crates/missiond-mcp/src/tools/comm/question.rs"
             "scripts/check-v3-incident-governance-isomorphism.mjs"]
      :note "Code-aligned V3 destination for question, incident, LLM trace, Gemini auth, and decision stats behavior. question.rs is the thin incident-governance facade; question/question_flow.rs owns mission_question create/list/get/answer/dismiss, running-autopilot task inference, QuestionEvent::Created/Resolved, and TaskEvent::Completed scheduler wakeup; question/decision.rs owns mission_decision_stats; question/llm_trace.rs owns mission_llm_trace plus legacy Gemini/Jarvis trace aliases, Gemini request log/stat/content reads, and Gemini watch lifecycle, with the watch probe model projected from router-runtime-policy flow-gemini-model through RouterRuntimeConfig; question/auth.rs owns mission_gemini_auth llm.yaml/settings.json projection; question/incident.rs owns mission_incident routing plus legacy mission_incident_* execution, incident injection/list/get/remediate/status/close, triage remediations, and safe close audit; handlers/mod.rs sends consolidated and legacy question/incident/LLM trace public tools through this facade.")

    (surface decision-inbox-revalidation
      :status "code-aligned"
      :implements [decision-inbox-revalidation stale-decision-revalidation]
      :code [".missiond/v3/missiond-blueprint.lisp"
             ".missiond/frontend/board-blueprint.lisp"
             "crates/missiond-daemon/src/handlers/comm/question/question_flow.rs"
             "crates/missiond-daemon/src/engine/lisp_code_sync.rs"
             "packages/board/src/components/PendingQuestions.tsx"
             "packages/board/src/types.ts"
             "scripts/check-v3-lisp-code-sync-isomorphism.mjs"]
      :note "decision-inbox-revalidation makes operational questions revalidate their facts before reaching the user. mission_question list/get can classify stale lisp-code-sync self-loop questions, read authoritative runtime status such as reportDirs and recentReports5m, answer obsolete items as stale_evidence/resolved_by_runtime_fix, close the linked stale operational BoardTask as done instead of reopening it, emit QuestionEvent::Resolved, and return revalidationStatus/evidenceFreshAt fields for still-valid questions. The Board frontend displays this status instead of treating old incident text as fresh truth.")

    (surface capability-governance
      :status "code-aligned"
      :implements [capability-usage audit codex-ops mcp-tool-governance]
      :code ["crates/missiond-mcp/src/tools/comm/capability_usage.rs"
             "crates/missiond-mcp/src/tools/comm/audit.rs"
             "crates/missiond-mcp/src/tools/comm/codex_ops.rs"
             "crates/missiond-mcp/src/tools/comm/tool_directory.rs"
             "crates/missiond-daemon/src/handlers/mod.rs"
             "crates/missiond-daemon/src/context/v3_blueprint_runtime.rs"
             "crates/missiond-daemon/src/handlers/comm/capability_usage.rs"
             "crates/missiond-daemon/src/handlers/comm/capability_usage/runtime.rs"
             "crates/missiond-daemon/src/handlers/comm/audit.rs"
             "crates/missiond-daemon/src/handlers/comm/codex_ops.rs"
             "crates/missiond-daemon/src/handlers/comm/tool_directory.rs"
             "scripts/check-v3-capability-governance-isomorphism.mjs"]
      :note "Runtime-projected V3 destination for capability usage, audit, Codex ops, and MCP tool-family governance surfaces. capability_usage.rs is the thin capability-governance facade; capability_usage/runtime.rs owns snapshot/report/candidates/mark/ack, six source lanes, semantic hint merge review, protected source/target policy, review sidecar persistence, and non-blocking observability emissions; context/v3_blueprint_runtime.rs projects capability-governance-policy review sidecar path plus protected source/target lists into mission_capability_usage runtime; audit.rs owns mission_audit trace/detail/stats/export plus legacy mission_audit_* compatibility; codex_ops.rs owns mission_codex_ops recent/thread/tool_stats over codex_cli conversations; tool_directory.rs owns mission_tool_directory list/recommend/lookup/explain/deprecated so agents can select primary families before raw compatibility tools.")

    (surface compute-primitives
      :status "code-aligned"
      :implements [pty task job flow-run process cc forge worker-control]
      :code ["crates/missiond-daemon/src/handlers/mod.rs"
             "crates/missiond-daemon/src/handlers/compute/mod.rs"
             "crates/missiond-daemon/src/handlers/compute/task.rs"
             "crates/missiond-daemon/src/handlers/compute/job.rs"
             "crates/missiond-daemon/src/handlers/compute/flow_run.rs"
             "crates/missiond-daemon/src/engine/flow/mod.rs"
             "crates/missiond-daemon/src/engine/flow/loader.rs"
             "crates/missiond-daemon/src/handlers/compute/pty.rs"
             "crates/missiond-pty/src/pty_recognition.rs"
             "crates/missiond-pty/src/session.rs"
             "crates/missiond-pty/src/manager.rs"
             "crates/missiond-daemon/src/handlers/compute/process.rs"
             "crates/missiond-daemon/src/handlers/compute/slot.rs"
             "crates/missiond-daemon/src/handlers/compute/minimax.rs"
             "crates/missiond-daemon/src/llm/minimax_client.rs"
             "crates/missiond-daemon/src/llm/minimax_gateway.rs"
             "crates/missiond-daemon/src/handlers/compute/cc_tasks.rs"
             "crates/missiond-daemon/src/handlers/compute/worker.rs"
             "crates/missiond-daemon/src/handlers/compute/forge.rs"
             "crates/missiond-daemon/src/context/v3_blueprint_runtime.rs"
             "crates/missiond-mcp/src/tools/compute/task.rs"
             "crates/missiond-mcp/src/tools/compute/job.rs"
             "crates/missiond-mcp/src/tools/compute/flow_run.rs"
             "crates/missiond-mcp/src/tools/compute/pty.rs"
             "crates/missiond-mcp/src/tools/compute/process.rs"
             "crates/missiond-mcp/src/tools/compute/slot.rs"
             "crates/missiond-mcp/src/tools/compute/minimax.rs"
             "crates/missiond-mcp/src/tools/compute/cc_tasks.rs"
             "crates/missiond-mcp/src/tools/compute/worker.rs"
             "crates/missiond-mcp/src/tools/compute/forge.rs"
             "scripts/check-v3-compute-primitives-isomorphism.mjs"
             "scripts/check-v3-pty-recognition-isomorphism.mjs"]
      :note "Code-aligned V3 destination for low-level worker runtime primitives. task.rs owns mission_task_submit/query/cancel plus async/sync/status/list/ack/track and TaskEvent::Created egress; mission_task_query bridges legacy tasks with BoardTask-backed delegated workers so running BoardTasks are visible to master/control callers; task.rs projects auto-spawn tracked PTY wait_for_idle timeout from compute-runtime-policy; job.rs owns mission_job_poll poll/list/cancel over AsyncJobStatus; flow_run.rs owns mission_flow_run BoardTask-backed flow execution and project-root resolution; engine/flow/mod.rs owns FlowDefinition shape constants, engine/flow/loader.rs loads flow-runtime-policy through context/v3_blueprint_runtime.rs and projects missing YAML node defaults while preserving explicit fields; pty.rs owns mission_pty_spawn/send/read/signal/confirm/status/screenshot plus kill/interrupt/read screen-history-logs, task requeue, and permission learning; process.rs owns mission_agent spawn/kill/restart/list and projects trac..."
      :evidence-sidecar ".missiond/v3/evidence/blueprint-notes.lisp#note-019")

    (surface skill-runtime
      :status "code-aligned"
      :implements [skill-query skill-context skill-operational-facts skill-mutate skill-exec]
      :code ["crates/missiond-daemon/src/handlers/knowledge/skill.rs"
             "crates/missiond-daemon/src/handlers/knowledge/skill/query.rs"
             "crates/missiond-daemon/src/handlers/knowledge/skill/context.rs"
             "crates/missiond-daemon/src/handlers/knowledge/skill/mutate.rs"
             "crates/missiond-daemon/src/handlers/knowledge/skill/exec.rs"
             "crates/missiond-mcp/src/tools/knowledge/skill.rs"
             "scripts/check-v3-skill-runtime-isomorphism.mjs"]
      :note "skill-runtime is the code-aligned surface for skill query/context/mutate/exec. skill.rs is the thin facade; query/context/mutate/exec modules own FTS/vector lookup, project skill links, opt-in KB aggregation, skill-derived operational_facts, mutation rollback/materialization, and explicit approve=true replay for requires_approval workflows."
      :evidence-sidecar ".missiond/v3/evidence/blueprint-notes.lisp#note-022")

    (surface cascade-governance
      :status "code-aligned"
      :implements [universe-graph cascade-plan cascade-trigger cascade-lint]
      :code ["crates/missiond-daemon/src/context/v3_blueprint_runtime.rs"
             "crates/missiond-daemon/src/handlers/knowledge/cascade.rs"
             "crates/missiond-daemon/src/handlers/knowledge/cascade/path.rs"
             "crates/missiond-daemon/src/handlers/knowledge/cascade/graph.rs"
             "crates/missiond-daemon/src/handlers/knowledge/cascade/plan.rs"
             "crates/missiond-daemon/src/handlers/knowledge/cascade/trigger.rs"
             "crates/missiond-daemon/src/handlers/knowledge/cascade/lint.rs"
             "crates/missiond-mcp/src/tools/knowledge/cascade.rs"
             "scripts/check-v3-cascade-governance-isomorphism.mjs"]
      :note "Code-aligned V3 destination for universe graph and cascade tools. cascade.rs is the thin cascade-governance facade; cascade/path.rs owns manifest/root path policy by loading CascadeRuntimeConfig from V3 cascade-policy before honoring explicit UNIVERSE_MANIFEST / UNIVERSE_ROOT overrides; cascade/graph.rs owns mission_universe_graph; cascade/plan.rs owns mission_cascade_plan dry-run; cascade/trigger.rs owns mission_cascade_trigger, V3 trigger-enabled plus CASCADE_TRIGGER_ENABLED explicit override, TaskEvent::CascadeTriggered/Completed, max-cycle clamp, and spawn_blocking execute_plan; cascade/lint.rs owns mission_cascade_lint integrity egress.")

    (surface sysinfra-control
      :status "code-aligned"
      :implements [sysinfra permission power daemon-update global-instruction]
      :code ["crates/missiond-daemon/src/handlers/mod.rs"
             "crates/missiond-daemon/src/handlers/sysinfra/mod.rs"
             "crates/missiond-daemon/src/handlers/sysinfra/infra.rs"
             "crates/missiond-daemon/src/handlers/sysinfra/permission.rs"
             "crates/missiond-daemon/src/handlers/sysinfra/power.rs"
             "crates/missiond-daemon/src/handlers/sysinfra/system.rs"
             "crates/missiond-daemon/src/handlers/sysinfra/global_instruction.rs"
             "crates/missiond-mcp/src/tools/sysinfra/infra.rs"
             "crates/missiond-mcp/src/tools/sysinfra/permission.rs"
             "crates/missiond-mcp/src/tools/sysinfra/power.rs"
             "crates/missiond-mcp/src/tools/sysinfra/system.rs"
             "crates/missiond-mcp/src/tools/sysinfra/global_instruction.rs"
             "scripts/check-v3-sysinfra-control-isomorphism.mjs"
             "scripts/check-v3-infrastructure-universe-isomorphism.mjs"]
      :note "Code-aligned V3 sysinfra surface. infra.rs owns infra query/ops, skill evidence, credential refs, and runtime target projection; project/reconcile reports runtime and credential drift. permission, power, system, and global-instruction handlers own their MCP tools. Learned permissions are scoped, non-blanket for Bash, TTL-governed with expires_at/source_evidence/renew_policy/audit_trail, and renewed only from provider-confirmation use. Long blue-green/self-update and infra-evidence anchors live in blueprint-notes#note-021.")

    (surface runtime-load-explanation
      :status "code-aligned"
      :implements [runtime-load-explanation runtimeLoadExplanation]
      :code ["crates/missiond-daemon/src/engine/master_control.rs"
             "crates/missiond-daemon/src/infra/daemon_stats.rs"
             "crates/missiond-daemon/src/engine/lisp_code_sync.rs"
             "crates/missiond-daemon/src/engine/shared_memory.rs"
             ".missiond/v3/missiond-blueprint.lisp"
             "scripts/check-v3-lisp-code-sync-isomorphism.mjs"]
      :note "runtime-load-explanation is the operator-facing explanation layer for MissionD internal load. It does not pretend to replace OS CPU sampling; it combines daemon stats, lisp-code-sync report counters, shared-memory workflow/cursor/claim counters, and nightly-evolution counters into runtimeLoadExplanation suspects so the Board and master can distinguish lisp-code-sync, EventBus backlog, workflow runner/shared-memory, context-prefetch, Autopilot/DB, or nightly-evolution activity before asking the user to decide.")

    (surface ops-infra
      :status "code-aligned"
      :implements [ops-infra]
      :code ["scripts/deploy-daemon.sh"
             "scripts/rustfmt-missiond.sh"
             "scripts/cargo-fmt-touched.sh"
             "crates/missiond-daemon/src/main.rs"
             "crates/missiond-daemon/src/workers/local/ast_sync_worker.rs"
             "scripts/check-v3-ops-infra-isomorphism.mjs"
             "scripts/check-missiond-blue-green-deploy.mjs"]
      :note "ops-infra owns deploy-daemon.sh plus formatter-converged Rust hygiene and restart-time background CPU policy. deploy-daemon.sh builds paired missiond/mission-mcp release candidates under ~/.xjp-mission/releases/<release-id>, writes release-manifest.json, switches ~/.xjp-mission/active, keeps stable entrypoints through active, kickstarts launchd, runs MCP smoke, rolls back to previous active, cleans retained releases, removes incomplete release dirs, discovers common managed-node Node.js and OCaml/opam install paths before typed Lisp runtime compile, and defaults deploy builds to CARGO_INCREMENTAL=0 so debug release updates do not fill target/debug/incremental. rustfmt-missiond.sh is the M6 repository formatter gate for MissionD-owned crates/** and rejects Rust source formatter exemption markers. cargo-fmt-touched.sh remains a scoped fallback for external/non-M6 project waves and touched-file checks. main.rs keeps repository-wide AST startup full sync opt-in via MISSIOND_AST_FULL_SYNC_ON_STARTUP, and ast_sync_worker skips topology KB rewrites when no stale files were synced.")

    (surface missiond-blue-green-self-update
      :status "code-aligned"
      :implements [blue-green-self-update release-manifest release-cleanup rollback]
      :code ["scripts/deploy-daemon.sh"
             "scripts/check-missiond-blue-green-deploy.mjs"
             "scripts/check-v3-ops-infra-isomorphism.mjs"
             "scripts/check-v3-sysinfra-control-isomorphism.mjs"]
      :note "MissionD self-update is owned as a blue-green release workflow. Release candidates are immutable directories under ~/.xjp-mission/releases/<release-id>; the active symlink is the only switch; daemon and MCP entrypoints both resolve through active so they share one release-manifest.json. The deploy path compiles typed Lisp runtime projections via node scripts/compile-v3-runtime.mjs --json before building binaries, records compiled projection schema/source/file hashes in typed_lisp_runtime, supports legacy direct-binary migration, verified linker signature acceptance before force-sign fallback, pre-switch MCP smoke, post-switch daemon IPC smoke, previous-release rollback, cleanup-only dry-run/apply, removal of incomplete release dirs, retention of active/previous/newest releases, and CARGO_INCREMENTAL=0 by default to keep self-update disk-bounded.")
    )
