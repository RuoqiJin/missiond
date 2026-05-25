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

(surface interaction-gateway
      :status "code-aligned"
      :implements [interaction-gateway channel-adapter identity-resolution permission-context intent-plan-routing response-sink interaction-audit]
      :code ["crates/missiond-core/src/ws/server.rs"
             "crates/missiond-mcp/src/tools/comm/interaction.rs"
             "crates/missiond-daemon/src/handlers/comm/interaction.rs"
             "crates/missiond-mcp/src/gen_gateway.rs"
             "scripts/check-v3-interaction-gateway-isomorphism.mjs"]
      :public-routes ["/interactions/v1/messages"
                      "/interactions/v1/{interaction_id}/events"
	                      "/jarvis/v1/chat/completions"
	                      "/jarvis/api/readiness"
	                      "/jarvis/api/monitor/jarvis"]
	      :internal-routes ["/internal/jarvis/slot/ensure"]
	      :smoke-scripts ["scripts/smoke-jarvis-chain.mjs"
	                      "scripts/smoke-jarvis-interaction.mjs"]
	      :slot-auto-heal (:env MISSIOND_JARVIS_SLOT_AUTO_HEAL
	                       :timeout-env MISSIOND_JARVIS_SLOT_AUTO_HEAL_TIMEOUT_SECS
	                       :rule "monitor is read-only; chat request and localhost-only /internal/jarvis/slot/ensure may restart default Exited/Error slot once and must fail fast with typed diagnostic if restart fails")
      :public-tools [mission_interaction]
      :auth-boundary (:authority auth
                      :userinfo-endpoint "/oidc/userinfo"
                      :endpoint-env MISSIOND_INTERACTION_AUTH_USERINFO_URL
                      :service-token-env MISSIOND_INTERACTION_SERVICE_TOKEN
                      :smoke-secret-ref-env MISSIOND_JARVIS_SMOKE_SECRET_REF
                      :service-token-ref "secret-store:missiond.jarvis-smoke/INTERACTION_SERVICE_TOKEN"
                      :timeout-env MISSIOND_INTERACTION_AUTH_TIMEOUT_MS
                      :failure-code INTERACTION_AUTH_UNAVAILABLE
                      :rule "Web/iOS/Jarvis bearer tokens must be resolved through Auth userinfo before PermissionContext is accepted. Automated Jarvis smoke uses MISSIOND_INTERACTION_SERVICE_TOKEN injected from secret-store ref missiond.jarvis-smoke/INTERACTION_SERVICE_TOKEN, or scripts/smoke-jarvis-interaction.mjs reads that same ref via MISSIOND_JARVIS_SMOKE_SECRET_REF; never use a fake token or repo-stored value. Deploy persists interaction auth env keys into launchd only when supplied by the operator environment. Metadata roles/tenant/application fields are hints only; MissionD must fail fast when Auth is unavailable instead of creating BoardTask side effects.")
      :legacy-adapter (:route "/v1/chat/completions"
                       :adapter handle_chat_completions_interaction_adapter
                       :normalizer openai_request_to_interaction_envelope
                       :rule "OpenAI-compatible Jarvis/iOS clients are wire-compatible only at the edge. The route must normalize into InteractionEnvelope and pass through Auth PermissionContext, grounding, intent/plan confirmation, BoardTask metadata, and task-result-artifact; it must not directly write provider PTYs.")
      :public-prefix (:route "/jarvis"
                      :normalizer normalize_public_jarvis_path
                      :rule "The daemon HTTP demux must accept canonical public /jarvis/* paths and normalize them to internal routes before WebSocket handshake fallback. Public monitor/readiness/chat requests must never be misclassified as WebSocket traffic when the auth proxy preserves the /jarvis prefix.")
      :events [received authenticated permission_resolved grounding intent_draft plan_draft confirm_required board_task_created worker_status result_artifact diagnostic final]
      :note "Unified external channel entry for Web, iOS, Jarvis, WeChat bridge, and service triggers. The HTTP adapter converts every external message to InteractionEnvelope, resolves Auth-derived PermissionContext, persists grounding through mission_context_gather, writes intent/plan artifacts, requires confirmation for broad human requests, creates BoardTasks only after confirmation, and returns status/result through channel response sinks. mission_interaction is the MCP facade for receive/confirm_intent/confirm_plan/follow/status; legacy Jarvis chat remains wire-compatible but is treated as a channel adapter, not a direct PTY path.")

(surface file-artifacts
      :status "code-aligned"
      :implements [file-artifacts request-local-artifacts compat-artifact-paths]
      :code ["crates/missiond-daemon/src/handlers/knowledge/file_artifacts.rs"
             "crates/missiond-daemon/src/handlers/knowledge/file_artifacts/attempt.rs"
             "crates/missiond-daemon/src/handlers/knowledge/file_artifacts/commit.rs"
             "crates/missiond-daemon/src/handlers/knowledge/file_artifacts/kind.rs"
             "crates/missiond-daemon/src/handlers/knowledge/file_artifacts/write.rs"
             "crates/missiond-core/src/db/artifact_commit.rs"
             "crates/missiond-core/src/db/pg/artifact_commit.rs"
             "crates/missiond-core/migrations/20260523001000_artifact_commit_outbox.sql"
             "crates/missiond-daemon/src/handlers/knowledge/file_artifacts/tests.rs"
             "scripts/check-v3-file-artifacts-isomorphism.mjs"]
      :note "file-artifacts is the shared writer layer for file-first Lisp artifacts and request-local artifact projection. V3 physical split: file_artifacts.rs is the facade; kind.rs owns ArtifactKind, artifact_path, sanitize_topic_segment, and compat path roots .missiond/alignment, .missiond/plans, and .missiond/workflows; write.rs owns atomic_write_artifact, read_existing_metadata, temp-file + fsync + rename; attempt.rs owns pure file writes and project-root resolution; commit.rs owns ArtifactCommitEnvelope and artifact_commit_outbox recovery for artifact/DB/event-coupled writes. mission_request start/respond and request-local plan materialization use operation_key idempotency. The invariant is no partial writes: failed writes must not leak partial bytes, and callers surface write_failed / partial status rather than pretending the Lisp artifact is authoritative.")

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
      :public-facade "mission_plan(action=compile|approve|mark|supersede|execute|record_evidence|...) remains wire-compatible; split contracts are internal ownership surfaces."
      :internal-surfaces [mission_plan-facade
                          plan-authoring-contract
                          plan-review-contract
                          plan-field-inference-contract
                          plan-execution-contract
                          plan-dag-contract
                          plan-rollback-contract
                          plan-task-runner-contract]
      :contract-split ((mission_plan-facade
                         :owns ["crates/missiond-daemon/src/handlers/knowledge/plan.rs"
                                "crates/missiond-mcp/src/tools/knowledge/plan.rs"])
                        (plan-authoring-contract
                         :owns ["crates/missiond-daemon/src/handlers/knowledge/plan/compile_authoring.rs"
                                "crates/missiond-daemon/src/handlers/knowledge/plan/compile_authoring/artifact.rs"
                                "crates/missiond-daemon/src/handlers/knowledge/plan/compile_authoring/validation.rs"])
                        (plan-review-contract
                         :owns ["crates/missiond-daemon/src/handlers/knowledge/plan/approval_review.rs"
                                "crates/missiond-daemon/src/handlers/knowledge/plan/approval_review/approve.rs"
                                "crates/missiond-daemon/src/handlers/knowledge/plan/approval_review/mark.rs"
                                "crates/missiond-daemon/src/handlers/knowledge/plan/approval_review/supersede.rs"])
                        (plan-field-inference-contract
                         :owns ["crates/missiond-daemon/src/handlers/knowledge/plan/field_inference.rs"
                                "crates/missiond-daemon/src/handlers/knowledge/plan/field_inference/apply.rs"
                                "crates/missiond-daemon/src/handlers/knowledge/plan/field_inference/llm.rs"])
                        (plan-execution-contract
                         :owns ["crates/missiond-daemon/src/handlers/knowledge/plan/execution_runtime.rs"
                                "crates/missiond-daemon/src/handlers/knowledge/plan/execution_runtime/internal.rs"
                                "crates/missiond-daemon/src/handlers/knowledge/plan/internal_dispatch.rs"
                                "crates/missiond-daemon/src/handlers/knowledge/plan/execute_hints.rs"])
                        (plan-dag-contract
                         :owns ["crates/missiond-daemon/src/handlers/knowledge/plan_dag.rs"
                                "crates/missiond-daemon/src/handlers/knowledge/plan_dag/runtime.rs"
                                "crates/missiond-daemon/src/handlers/knowledge/plan_dag/parser.rs"])
                        (plan-rollback-contract
                         :owns ["crates/missiond-daemon/src/handlers/knowledge/plan_dag/rollback.rs"
                                "crates/missiond-daemon/src/handlers/knowledge/plan_dag/rollback/cascade.rs"])
                        (plan-task-runner-contract
                         :owns ["crates/missiond-daemon/src/handlers/knowledge/plan/task_runner_dry_run.rs"
                                "crates/missiond-daemon/src/handlers/knowledge/plan/task_runner_dry_run/manifest.rs"
                                "crates/missiond-daemon/src/handlers/knowledge/plan/task_runner_dry_run/projection.rs"]))
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
      :note "compiler_mode=dry_run now renders plan-draft as an executable Lisp scaffold with :target, :objective, and :nodes; compile/materialization persist plan.contract_json using missiond.plan-contract.v2 shape, execute derives target_source=plan_hint from plan.contract_json, and empty legacy rows are reprojected through missiond-lispc emit-plan-contract before dispatch."
      :evidence-sidecar ".missiond/v3/evidence/blueprint-notes.lisp#note-004")

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
)
