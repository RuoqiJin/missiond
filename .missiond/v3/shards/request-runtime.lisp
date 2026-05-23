  (grounding-search-aggregate
    :schema "missiond.grounding-search-aggregate.v1"
    :purpose "Provide one high-frequency fact-gathering entry before intent.lisp, plan.lisp, Board triage, deploy decisions, or worker delegation so operators do not have to remember every retrieval surface."
    :primary-tool mission_context_gather
    :default-sources [project-registry ssot-intent active-kb skill-operational-evidence infra-evidence active-board-task-records bounded-conversation-logs]
    :source-policy
      ((source active-board-task-records
         :tool mission_board_query
         :scope active
         :rule "Task records are retrieval evidence and must be searchable through FTS/embedding, but broad historical/done Board backlog is excluded unless include_historical=true.")
       (source bounded-conversation-logs
         :tool mission_conversation_query
         :scope query-scoped
         :default-time-range last_30d
         :rule "Durable provider/user conversations are searched only by explicit query/unknowns; this is not prompt preloading and does not re-enable broad historical log context.")
       (source skill-operational-evidence
         :tool mission_skill_context
         :rule "Skill files are operational evidence for ClaudeCode-compatible workers; mutation of skill files must be delegated through skill-edit-delegation-policy.")
       (source active-kb
         :tool mission_kb_query
         :rule "Default retrieval applies knowledge_review_state overlay and excludes archived/superseded/noise memories.")
       (source ssot-intent
         :tool mission_intent
         :rule "Active SSOT Lisp is the long-lived project fact authority; cold runtime evidence is opt-in."))
    :functions
      ((function context-gather-before-intent
         :entry [user-request BoardTaskCreated external-intent-envelope unknowns-inventory]
         :core ((step s1 :logic "ask unknowns-first: what facts are still missing before judging user intent?")
                (step s2 :logic "call mission_context_gather once with query/unknowns/project/skill/infra_target and default sources")
                (step s3 :logic "synthesize evidence_refs and remaining unknowns into intent.lisp review packet")
                (step s4 :logic "write high-confidence inferred user intent as memory:decision candidate only after evidence refs are attached"))
         :egress [context-gather-result intent-review-packet intent-memory-candidate])
       (function task-record-indexing
         :entry [BoardTask workflow_run task-result-artifact audit-event]
         :core ((step s1 :logic "index BoardTask title/description/status/project/category, workflow_run summary, task-result-artifact summary, and audit event captions into the memory provider search corpus")
                (step s2 :logic "dedupe by source_type/source_id and preserve source authority so Board noise does not become active KB memory")
                (step s3 :logic "make active task records searchable by mission_context_gather without preloading full Board backlog"))
         :egress [fts-document embedding-document retrieval-evidence-ref]))
    :invariants
      ["mission_context_gather MUST aggregate KB, active SSOT, project registry, skill operational evidence, infra evidence, active Board task records, and bounded conversation logs."
       "Board/task/workflow records are searchable retrieval evidence, not active long-term memory unless promoted by an explicit review workflow."
       "Conversation logs are searched by query and bounded window; they are not default prompt preloads."
       "If mission_context_gather cannot answer a source, it returns source-specific diagnostics instead of making the resident master guess."]
    :checker "node scripts/check-v3-memory-kb-isomorphism.mjs")

  (grounded-dispatch-policy
    :schema "missiond.grounded-dispatch-policy.v1"
    :purpose "Make unknowns-first context gathering a runtime gate before broad Board/Jarvis/master tasks can reach provider PTYs; prompt hints are not sufficient."
    :authority [mission_context_gather mission_task_delegate mission_swarm_run autopilot-runtime shared-memory task-result-artifact]
    :bypass-policy
      ((case exact-shard
         :requires [exact_shard_ready accepted_shard_id context_pack_path write_scope]
         :rule "Exact implementation shards already produced by intent/plan/workflow may dispatch without re-gathering, but must reference their accepted shard and scoped write surface.")
       (case emergency-code-first
         :requires [emergency_code_first visible-backfill-boardtask]
         :rule "Emergency code-first is allowed only as an explicit exception and must create Lisp/checker/evidence backfill."))
    :artifact
      ((kind context-gather-artifact
         :schema "missiond.context-gather-artifact.v1"
         :id-field grounding_context_id
         :storage "shared_artifacts(kind=context-gather)"
         :fields [unknowns query project_id sources_used evidence_refs diagnostics grounded_intent_summary context_pack_path]
         :rule "mission_context_gather(persist=true) returns grounding_context_id and context_pack_path; worker prompts receive only a small context slice, not broad KB/history preloads.")
       (kind task-result-artifact
         :schema "missiond.task-result-artifact.v1"
         :id-field artifact_hash
         :storage "shared_artifacts(kind=task-result)"
         :fields [task_id project_id provider result_status summary json source]
         :rule "Jarvis, Board, and worker completion surfaces MUST normalize summary notes/provider finals into task-result-artifact before streaming result_artifact/final to clients; Board notes, PTY text, and provider finals are projections/evidence, not the canonical result."))
    :functions
      ((function context-gather-artifact
         :entry [mission_context_gather unknowns-inventory BoardTask source_id project_id]
         :core ((step s1 :logic "derive query from explicit unknowns or the raw objective; never use broad historical preload as the query source")
                (step s2 :logic "query project registry, active SSOT, active KB, skill evidence, infra/deploy facts, active Board task records, bounded conversations, and tool directory through the aggregate")
                (step s3 :logic "return source-specific diagnostics for missing or stale authorities instead of letting the worker guess")
                (step s4 :logic "persist the payload into shared_artifacts(kind=context-gather) and return grounding_context_id plus shared-artifact context_pack_path"))
         :egress [grounding_context_id context_pack_path sources_used diagnostics shared_artifact])
       (function task-delegate-grounding-gate
         :entry [mission_task_delegate mission_swarm_run mission_plan_execute]
         :core ((step s1 :logic "classify dispatch as exact shard, emergency code-first, or broad objective")
                (step s2 :logic "for broad objective without grounding_context_id, synchronously call mission_context_gather(persist=true)")
                (step s3 :logic "fail fast with GROUNDING_REQUIRED if gather returns diagnostics or no grounding_context_id")
                (step s4 :logic "write grounding_context_id, context_pack_path, sources_used, and evidence count into BoardTask metadata and prompt slice")
                (step s5 :logic "implementation swarm lanes still require accepted_shard_id and write_scope; gathered broad context may only create investigation/synthesis tasks"))
         :egress [grounded-BoardTask delegation-metadata context-pack-slice GROUNDING_REQUIRED])
       (function autopilot-grounding-gate
         :entry [BoardTaskBeforeDispatch auto_execute task-metadata]
         :core ((step s1 :logic "allow exact shard only when accepted_shard_id, context_pack_path, and write_scope are present")
                (step s2 :logic "allow broad task only when grounding_context_id is present")
                (step s3 :logic "block ungrounded broad task before PTY input and append a diagnostic Board note")
                (step s4 :logic "never re-enable hidden context prefetch as a substitute for grounded dispatch"))
         :egress [BoardTaskBlocked diagnostic-note no-PTY-dispatch])
       (function jarvis-result-artifact-gate
         :entry [JarvisSSE BoardTaskSummaryNote provider-final task-result-artifact]
         :core ((step s1 :logic "inspect worker/Board summary notes for existing task-result-artifact hash")
                (step s2 :logic "when only a Board summary projection exists, write a canonical task-result-artifact through shared memory before emitting result_artifact")
                (step s3 :logic "bound artifact write latency with an explicit timeout and emit TASK_RESULT_ARTIFACT_WRITE_TIMEOUT when the writer stalls")
                (step s4 :logic "emit TASK_RESULT_ARTIFACT_WRITE_FAILED diagnostic instead of pretending a missing artifact exists")
                (step s5 :logic "if a BoardTask is done with a summary but no artifact hash, emit TASK_RESULT_ARTIFACT_REQUIRED and fail fast instead of streaming the Board note as final text")
                (step s6 :logic "stream final text only after the task-result artifact hash is known or the diagnostic is surfaced"))
         :egress [task-result-artifact result_artifact_event final_event diagnostic]))
    :invariants
      ["All non-exact worker dispatch must carry grounding_context_id before a provider PTY receives the prompt."
       "mission_context_gather is the only default aggregate for KB/SSOT/project/skill/infra/Board/conversation/tool facts; callers should not hand-roll partial context lookup."
       "Grounding artifacts are durable evidence and task metadata; hidden prompt preloads are not grounding."
       "Autopilot must block broad ungrounded BoardTasks instead of sending them to workers for self-discovery."
       "Direct local code search is allowed only after the grounding artifact identifies code surface evidence as a required source."
       "Jarvis dispatch metadata MUST derive read_scope from the active runtime/project root (MISSIOND_PROJECT_ROOT, MISSIOND_REPO_ROOT, MISSIOND_WORKSPACE_ROOT, or current daemon cwd) and MUST NOT hardcode a developer-machine root path."
       "Jarvis result streaming MUST use task-result-artifact as canonical completion authority; Board summary notes are converted to artifacts before client-visible result_artifact events."
       "Jarvis result artifact writes MUST be bounded; missing or stalled artifact writes produce typed diagnostics and MUST NOT silently fall back to Board note final text."]
    :checks ["node scripts/check-v3-grounded-dispatch-isomorphism.mjs --json"])

  (unified-entry
    :desc "request -> intent alignment -> plan -> execution -> evidence -> workflow"
    :modes
      ((mode human-interactive
         :requires [intent-review-gate plan-review-gate]
         :flow [request intent-alignment approve-intent plan approve-plan execute])
       (mode trusted-agent
         :requires [trusted-agent-policy risk-gate scoped-write-gate]
         :flow [request plan-with-intent policy-gate execute]
         :audit "intent alignment is embedded in plan.intent and preserved in lifecycle events"))
    :single-entry-surface mission_request
    :compat-surfaces [mission_directive mission_plan mission_workflow]
    :non-goal "Do not let clients bypass plan-runner by directly dispatching workstation work."
    (review-packet
      :desc "Compact projection of which artifact mission_request expects the human to review next; pure projection from request-local artifact existence + latest pipeline projection. Never auto-approves intent, never auto-dispatches plan."
      :surface mission_request
      :emitted-on [start advance status]
      :fields [:state :artifact_kind :artifact_path :artifact_exists
               :artifact_preview :prompt :allowed_responses :next_action
               :execute_allowed]
      :response-rule "start/advance/status/respond expose request-local :artifact_paths + :artifact_exists at the top level whenever a request_id can be resolved; callers do not need to inspect legacy pipeline file paths or nested wrappers to locate Lisp artifacts."
      :states [:received :intent_drafting :awaiting_intent_approval
               :awaiting_plan_approval :awaiting_execution :execute_requested]
      :state-derivation
        ((rule plan-present-wins
           :when "plan.lisp exists, execute was not explicitly requested, and the latest review event is not dispatched approve_plan"
           :state :awaiting_plan_approval
           :artifact_kind :plan
           :next_action "call mission_request respond with response=approve_plan / reject_plan / ask_question; execute later via response=execute_plan + execute=true"
           :execute_allowed false)
         (rule plan-present-execute-requested
           :when "plan.lisp exists and execute=true was passed on this call or the latest review event is dispatched execute_plan"
           :state :execute_requested
           :artifact_kind :plan
           :next_action "observe execution status through mission_request status and task receipts"
           :execute_allowed true)
         (rule plan-approved-event
           :when "plan.lisp exists and the latest review event is dispatched approve_plan"
           :state :awaiting_execution
           :artifact_kind :plan
           :next_action "call mission_request respond with response=execute_plan + execute=true"
           :execute_allowed true)
         (rule intent-only-present
           :when "intent-alignment.lisp exists and plan.lisp does not"
           :state :awaiting_intent_approval
           :artifact_kind :intent_alignment
           :next_action "call mission_request respond with response=approve_intent / reject_intent / ask_question"
           :execute_allowed false)
         (rule intent-drafting
           :when "neither intent-alignment.lisp nor plan.lisp exists, but projection just wrote one (target=intent_alignment|plan)"
           :state :intent_drafting
           :artifact_kind :intent_alignment
           :next_action "wait for projection to land, then re-poll mission_request status"
           :execute_allowed false)
         (rule received-default
           :when "no request-local artifacts and no projection target"
           :state :received
           :artifact_kind :request
           :next_action "call mission_request advance to drive the next pipeline stage"
           :execute_allowed false))
      :allowed-responses
        ((human-interactive
           :awaiting_intent_approval [approve_intent reject_intent ask_question]
           :awaiting_plan_approval [approve_plan reject_plan ask_question]
           :awaiting_execution [execute_plan ask_question]
           :execute_requested [observe]
           :default [observe])
         (trusted-agent
           :awaiting_intent_approval [approve_intent ask_question]
           :awaiting_plan_approval [approve_plan ask_question]
           :awaiting_execution [execute_plan ask_question]
           :execute_requested [observe]
           :default [observe]))
      :preview-policy
        (:source "request-local artifact bytes when artifact_exists; otherwise compiled_sexp_preview from latest projection"
         :max-bytes 480
         :truncation "missiond-core safe_byte_truncate (UTF-8 boundary safe)"
         :rationale "previews must never panic on multi-byte CJK runes")
      :non-goal "review_packet is pure observation; only an explicit review-response may call approval gates or plan-authoring, and no path dispatches workstation slots without execute_plan.")
    (review-response
      :desc "Caller continuation of a review_packet through mission_request. The user-facing surface answers a review_packet without learning the inner mission_directive / mission_plan calls; mission_request is the adapter that routes to the existing approval gates and never bypasses them or directly dispatches workstation work."
      :surface mission_request
      :action respond
      :inputs [:request_id :response :decision :note :board_task_id :execute
               :directive_id :approved_directive_id :directive_version
               :plan_id :approved_plan_id :project :cwd :target_project
               :target :dispatch_strategy :parallelism :objective :flow_id]
      :decisions [approve_intent reject_intent ask_question
                  approve_plan reject_plan execute_plan]
      :decision-routing
        ((rule approve-intent
           :requires [persisted-or-explicit-directive-ref]
           :route "delegate to mission_directive(action=approve, directive_id, version) using the existing approval gate; when approval succeeds, ensure a hidden BoardTask anchor if board_task_id was not supplied, then immediately continue through unified_entry s4 plan-authoring and project the resulting sexp into the same request-local plan.lisp; dry_run plan-authoring must include Lisp-native execution hints (:target, :objective, :nodes) so later execute_plan can route from plan.lisp rather than caller-supplied escape hatches"
           :default-board-task "board_task_id if supplied; otherwise create a hidden request-local BoardTask anchor so callers do not need internal board ids"
           :next_action "review request-local plan.lisp from the returned review_packet")
         (rule reject-intent
           :requires [persisted-or-explicit-directive-ref :note]
           :route "no DB mutation; record review event under .missiond/requests/<request_id>/events as auditable user decision"
           :next_action "revise the message and call mission_request start/advance again, or use mission_directive directly for explicit review_decision=rejected")
         (rule ask-question
           :requires [:note]
           :route "no DB mutation; record review event capturing the question text; orchestrator/UI surfaces it"
           :next_action "wait for follow-up answer, then call mission_request respond again with approve_intent / approve_plan")
         (rule approve-plan
           :requires [persisted-or-explicit-plan-ref request-local-plan-lisp]
           :route "when plan_id is explicit, parsed from plan.lisp, or recovered from a prior review event, delegate to mission_plan(action=approve, plan_id); when only request-local plan.lisp exists, materialize it into a draft Plan row first, reusing plan.lisp's BoardTask anchor when present or creating a hidden one only if needed, then approve through mission_plan; never sets execute=true"
           :next_action "call mission_request respond again with response=execute_plan + execute=true to dispatch the approved plan")
         (rule reject-plan
           :requires [persisted-or-explicit-plan-ref :note]
           :route "no DB mutation; record review event"
           :next_action "revise the plan and call mission_request advance, or use mission_plan directly for explicit review_decision=rejected")
         (rule execute-plan
           :requires [persisted-or-explicit-plan-ref :execute-true]
           :route "delegate to unified_entry::run_pipeline with approved_plan_id + execute=true so mission_plan execute path enforces the same scoped-write / risk gates"
           :guard "execute_plan requires execute=true (or response=execute_plan); a missing execute flag returns a structured blocked response, never a silent dispatch"))
      :ref-resolution
        (:order [explicit-arg artifact-extracted review-event-extracted request-local-materialized]
         :explicit-arg "callers may pass approved_directive_id / directive_id / approved_plan_id / plan_id directly"
         :artifact-extracted "request-local intent-alignment.lisp / plan.lisp is parsed for the persisted id when the explicit arg is omitted; artifact extraction trusts explicit :directive_id / :plan_id, and treats generic :id as a persisted directive/plan ref only when it is UUID-shaped so nested ids such as (:id \"root\") never become refs"
         :review-event-extracted "execute_plan can recover the plan_id from the latest request-local approve_plan review event"
         :request-local-materialized "approve_plan may materialize a persisted plan_id by inserting a draft Plan row from request-local plan.lisp, reusing its BoardTask anchor when present and creating a hidden request-local anchor only when needed; after materialization it writes the persisted ref back into request-local plan.lisp so the artifact is self-contained"
         :missing "when neither source yields or can materialize a persisted ref, return a structured blocked response with next_action describing how to obtain it; mission_request never fabricates non-persisted ids")
      :event-ledger
        (:path ".missiond/requests/<request_id>/events/<seq>.event.lisp"
         :schema "missiond.lifecycle-event.v1"
         :kinds [review_response_recorded review_response_dispatched review_response_blocked]
         :seq-allocation "monotonically increasing local sequence; allocator scans existing event files, picks max+1, and writes atomically — never overwrites an existing event"
         :writer "mission_request action=respond"
         :payload [:request_id :decision :note :directive_id :plan_id :execute :outcome])
      :response-shape
        (:respond_result {:decision :outcome :event_path :event_seq :next_action
                          :directive_id :plan_id :execute :inner_action :board_task_materialization :plan_materialization}
         :review_packet review-packet
         :projection "present on approve_intent when the follow-up plan compile projected plan.lisp"
         :board_task_materialization "present on approve_intent when request-local plan-authoring needed a hidden BoardTask anchor"
         :plan_materialization "present on approve_plan when request-local plan.lisp was promoted to a hidden BoardTask + draft Plan row"
         :pipeline_result "inner directive/plan/unified-entry payload when the route invoked one; approve_intent nests approval + plan_compile + projection; null for record-only routes"
         :next_action "human-readable continuation hint mirroring review_packet.next_action")
      :non-goals
        ("never auto-approves intent or plan when the user said reject/ask"
         "never spawns workstation work directly — execute_plan is a thin wrapper around mission_plan execute"
         "never invents a directive/plan id; missing-ref always returns blocked"
         "never edits the inner mission_directive / mission_plan / unified_entry handlers — adapter only"))
    (tool-schema-contract
      :surface mission_request
      :rule "The MCP input_schema is a projection of this Lisp review-response contract, not a permissive hidden bag; fields used for plan routing such as :target, :objective, :requested_cwd, :flow_id, :dispatch_strategy, :parallelism, :target_project, :cwd, :project, :execute_mode, :scheduler_mode, and :dry_run must be visible as explicit tool properties even when additionalProperties remains true for compatibility. The compatibility-writer switch :compat_write_file MUST be exposed as an explicit boolean property; legacy :write_file is preserved as an alias only."
      :implementation "crates/missiond-mcp/src/tools/knowledge/request.rs builds properties structurally to avoid serde_json::json! recursion limits as the Lisp contract grows.")
    (compat-writer-policy
      :surface mission_request
      :rule "Default mission_request flow projects only request-local artifacts (request.lisp, intent-alignment.lisp, plan.lisp, events/<seq>.event.lisp under .missiond/requests/<request_id>/). The legacy compatibility writers under .missiond/alignment/<topic>/ and .missiond/plans/<plan_id>/ MUST be opt-in: callers pass compat_write_file=true (V3 name) or legacy write_file=true (alias) to fire them. mission_request MUST NOT forward write_file=true to mission_directive or mission_plan unless one of those flags is explicitly true on the caller args."
      :v3-flag :compat_write_file
      :legacy-alias :write_file
      :default false
      :rationale "Wave43 evidence: live mission_request smoke that passed write_file=true left .missiond/alignment/<request_id>/ and .missiond/plans/<plan_id>/ artifacts in the worktree even after --cleanup, because the request-local cleanup scope intentionally excludes the compat roots. Defaulting compat off keeps the worktree request-local while preserving the legacy escape hatch for callers that depend on the old roots.")
    (execute-dry-run-smoke
      :surface mission_request
      :rule "Live mission_request smoke MUST keep the default --live-ipc path stopping at awaiting_execution; the only path that MAY call execute_plan from a checker is an explicit opt-in audit mode (preferred name --execute-dry-run on scripts/check-v3-request-flow-smoke.mjs). That audit path MUST drive the workstation-dispatch substrate end-to-end without spawning a slot: it MUST pass execute=true, dry_run=true, execute_mode=internal, dispatch_strategy=agent-team, and target=mission_task_delegate on the execute_plan respond call so mission_plan's `action_execute_internal` reaches `run_workstation_dispatch_with_contract_and_trace` and returns the `WorkstationDispatchOutcome::DryRun` shape: status=dry_run, execute_mode=internal, runner_status=workstation_dispatch_v0, workstation_dispatch_status=dry_run_no_dispatch, target_tool=mission_task_delegate, dispatch_strategy=agent-team, with task_brief_preview present. Bridge mode (status=bridge_ready, runner_status=bridge_only) is no longer accepted as a no-dispatch proof for --execute-dry-run because it bypasses the substrate; the audit must prove MissionD reached the workstation-dispatch substrate but emitted would_dispatch instead of dispatching. The smoke MUST NOT spawn or wait for a ClaudeCode worker."
      :audit-flag :--execute-dry-run
      :respond-args (:response :execute_plan
                     :execute true
                     :dry_run true
                     :execute_mode :internal
                     :dispatch_strategy :agent-team
                     :target :mission_task_delegate)
      :asserts [:respond_outcome_dispatched
                :respond_inner_action_unified_entry_plan_execute
                :respond_result_execute_true
                :review_packet_state_execute_requested
                :allowed_responses_observe_only
                :request_local_execute_plan_event_appended
                :pipeline_result_no_dispatch_proof
                :pipeline_execute_mode_internal
                :pipeline_runner_status_workstation_dispatch_v0
                :pipeline_workstation_dispatch_status_dry_run_no_dispatch
                :pipeline_target_tool_mission_task_delegate
                :pipeline_dispatch_strategy_agent_team
                :pipeline_task_brief_preview_present]
      :no-dispatch-proofs ((workstation-dispatch-substrate
                             :status "dry_run"
                             :execute_mode "internal"
                             :runner_status "workstation_dispatch_v0"
                             :workstation_dispatch_status "dry_run_no_dispatch"
                             :target_tool "mission_task_delegate"
                             :dispatch_strategy "agent-team"
                             :task_brief_preview :present))
      :non-goal "Default --live-ipc and the v3 aggregate gate MUST remain non-executing; the audit flag is opt-in for explicit smoke runs and never appears in check-v3-code-isomorphism-complete."
      :rationale "Wave45 proved mission_request can drive execute_plan without consuming a workstation slot, but the observed no-dispatch proof was bridge mode (status=bridge_ready / runner_status=bridge_only) — bridge mode short-circuits before the workstation_dispatch substrate runs, so it does not exercise `run_workstation_dispatch_with_contract_and_trace`, evidence emission, or task_brief rendering. Wave46 tightens the audit so the smoke explicitly drives execute_mode=internal + dispatch_strategy=agent-team, satisfying `evaluate_dispatch_decision`'s auto-inference (target=mission_task_delegate + INFERABLE strategy + non-empty objective + cwd as scoping signal) and forcing the substrate path. The expected outcome `WorkstationDispatchOutcome::DryRun` builds the brief, skips the inner tool, and returns `workstation_dispatch_status=dry_run_no_dispatch` with `task_brief_preview` populated. This proves MissionD reached the workstation-dispatch substrate without spawning a slot.")
    (real-dispatch-smoke
      :surface mission_request
      :rule "Real dispatch through mission_request execute_plan is slow + side-effecting (it creates a delegated BoardTask and may auto-provision a worker slot via mission_task_delegate). It MUST stay behind a separate, deliberately named opt-in flag (preferred name --execute-real-dispatch on scripts/check-v3-request-flow-smoke.mjs) and MUST NOT appear in default --live-ipc, --execute-dry-run, or check-v3-code-isomorphism-complete. The opt-in audit MUST pass execute=true, dry_run=false (or omit dry_run), execute_mode=internal, dispatch_strategy=agent-team, target=mission_task_delegate, cwd=<repo>, and a smoke objective that explicitly tells the delegated worker to do no file edits and no commits (read-only smoke; classify_task_kind→ReadOnly with empty owned_files so the brief instructs commit_status=not-required). The substrate (run_workstation_dispatch_with_contract_and_trace) MUST take the `WorkstationDispatchOutcome::Dispatched` branch and the response MUST surface: pipeline_result.status=executing (the plan FSM transitions to Executing on a successful Dispatched outcome — see plan.rs::build_workstation_dispatch_response), execute_mode=internal, runner_status=workstation_dispatch_v0, workstation_dispatch_status=dispatched (the substrate-level dispatch invariant emitted by outcome_to_response_fields), target_tool=mission_task_delegate, dispatch_strategy=agent-team, task_brief_preview present (non-empty), inner_result present and non-null, and a stable delegated BoardTask UUID at pipeline_result.delegated_board_task_id (projected by workstation_dispatch/outcome.rs::extract_inner_board_task_id from the inner mission_task_delegate response, which currently embeds the full BoardTask row at inner_result.task_id because compute/task_delegate.rs::handle shadows the variable name). The smoke MUST NOT wait synchronously for the delegated worker to finish; if a wait/observe mode is offered it MUST be a SECOND, separately gated, bounded option (not the default of --execute-real-dispatch). Filesystem cleanup is request-local only: --cleanup may remove .missiond/requests/<request_id>/ but MUST NOT delete the delegated BoardTask row, audit rows, or any worker-side artifacts. The checker MUST report delegated_board_task_id and the observed BoardTask status so the parent / Autopilot can observe or close the BoardTask."
      :completion-log-rule "Live workstation dispatch MUST pre-open the companion MissionD execution log before mission_task_delegate receives the brief. The brief pins `execution_id=\"plan-<plan_id>\"`, and read-only workers may append only that audit log when calling mission_execution(action=complete); if the log cannot be opened, dispatch returns skipped_completion_log_unavailable instead of handing the worker an impossible completion contract."
      :audit-flag :--execute-real-dispatch
      :respond-args (:response :execute_plan
                     :execute true
                     :dry_run false
                     :execute_mode :internal
                     :dispatch_strategy :agent-team
                     :target :mission_task_delegate
                     :cwd :repo-root
                     :objective :no-edit-no-commit-smoke)
      :asserts [:respond_outcome_dispatched
                :respond_inner_action_unified_entry_plan_execute
                :respond_result_execute_true
                :review_packet_state_execute_requested
                :allowed_responses_observe_only
                :request_local_execute_plan_event_appended
                :pipeline_status_executing
                :pipeline_execute_mode_internal
                :pipeline_runner_status_workstation_dispatch_v0
                :pipeline_workstation_dispatch_status_dispatched
                :pipeline_target_tool_mission_task_delegate
                :pipeline_dispatch_strategy_agent_team
                :pipeline_task_brief_preview_present
                :pipeline_inner_result_present
                :pipeline_delegated_board_task_id_uuid]
      :dispatch-proof ((workstation-dispatch-substrate
                         :status "executing"
                         :execute_mode "internal"
                         :runner_status "workstation_dispatch_v0"
                         :workstation_dispatch_status "dispatched"
                         :target_tool "mission_task_delegate"
                         :dispatch_strategy "agent-team"
                         :task_brief_preview :present
                         :inner_result :present
                         :delegated_board_task_id :uuid))
      :rust-projection-source "crates/missiond-daemon/src/handlers/knowledge/workstation_dispatch/outcome.rs::extract_inner_board_task_id"
      :non-goal "Default --live-ipc, --execute-dry-run, and check-v3-code-isomorphism-complete MUST remain non-real-dispatching. --execute-real-dispatch is the SOLE entry point that creates a delegated BoardTask + (optionally) auto-provisions a worker slot. CI MUST NOT block on the delegated worker finishing — the smoke validates creation/response shape only and leaves the BoardTask for Autopilot to drive."
      :rationale "Wave46 proved the workstation_dispatch substrate accepts execute_mode=internal + dry_run=true and returns `WorkstationDispatchOutcome::DryRun` with workstation_dispatch_status=dry_run_no_dispatch — the LAST step before a real dispatch. Wave47 closes the loop by exercising the same substrate with dry_run=false: `run_workstation_dispatch_with_contract_and_trace` builds the brief, calls mission_task_delegate (which auto-creates a BoardTask via state.store.create_board_task and notifies the dispatcher), and returns `WorkstationDispatchOutcome::Dispatched { inner_payload, .. }`. The minimal Rust projection (extract_inner_board_task_id) surfaces a stable `delegated_board_task_id` UUID at the top level of pipeline_result without rewriting compute/task_delegate.rs (which is outside this wave's write scope). Two deviations from the brief's draft assertions, both documented above and reflecting the established daemon shape: (a) pipeline_result.status='executing' (NOT 'dispatched') because the wave-15 substrate response intentionally surfaces the FSM transition; the substrate-level dispatch invariant lives at workstation_dispatch_status='dispatched'. (b) the BoardTask UUID is exposed via the new top-level `delegated_board_task_id` field rather than `inner_result.task_id` (which today contains the full DB row, not a string). This is intentionally the only checker that may consume a real workstation slot; gating it behind a deliberately-named opt-in flag (rather than overloading --confirm-execute) prevents accidental real dispatch from CI or daemon-free runs."))

  (state-machines
    (state-machine unified-entry
      :initial :received
      :terminal [:done :failed :blocked]
      (transition :received -> :intent_drafted
        :actor alignment-author
        :writes [intent-alignment]
        :when "mode=human-interactive")
      (transition :intent_drafted -> :awaiting_intent_approval
        :actor orchestrator
        :emits [:review_question_created])
      (transition :awaiting_intent_approval -> :intent_approved
        :gate intent-review-gate)
      (transition :intent_approved -> :plan_drafted
        :actor plan-author
        :writes [plan])
      (transition :plan_drafted -> :awaiting_plan_approval
        :actor orchestrator
        :emits [:review_question_created])
      (transition :awaiting_plan_approval -> :plan_approved
        :gate plan-review-gate)
      (transition :received -> :plan_drafted
        :actor trusted-agent
        :writes [plan]
        :when "mode=trusted-agent and policy allows folded intent")
      (transition :plan_approved -> :executing
        :actor plan-runner
        :writes [lifecycle-event])
      (transition :executing -> :verifying
        :actor verifier
        :writes [verification-receipt])
      (transition :verifying -> :done
        :actor finalizer
        :writes [final-report]))

    (state-machine execution-lifecycle
      :initial :planned
      :terminal [:complete :failed :abandoned :superseded]
      :derived-from lifecycle-event
      :states [:planned :dispatchable :dispatched :claimed :running
               :worker_committed :draft_reported :parent_patched
               :verification_pending :verified :report_finalized :complete
               :blocked :stale :failed :abandoned :superseded]
      :completion-rule "complete iff final-report is finalized, final commit matches lineage, and required receipts are valid")

    (state-machine delegated-boardtask-runtime
      :initial :queued
      :terminal [:done :blocked :failed :skipped]
      :derived-from [BoardEvent SlotEvent ExecutionEvent]
      :states [:queued :event_woken :eligible :claimed :slot_selected
               :prompt_sent :running :completed :completion_audited
               :done :blocked :failed :skipped]
      (transition :queued -> :event_woken
        :event [BoardEvent::TaskCreated SlotEvent::BecameIdle]
        :actor event-bus-subscriber
        :effect "notify dedicated Autopilot dispatch task without running pty.send inline")
      (transition :event_woken -> :eligible
        :actor autopilot-runtime
        :reads [board_task dependency_state slot_state global_pause])
      (transition :eligible -> :claimed
        :actor autopilot-runtime
        :writes [board_claim lease])
      (transition :claimed -> :slot_selected
        :actor autopilot-runtime
        :writes [assignee dispatch_guard])
      (transition :slot_selected -> :prompt_sent
        :actor autopilot-runtime
        :emits [SlotEvent::TaskDispatched])
      (transition :prompt_sent -> :running
        :actor worker-slot)
      (transition :running -> :completed
        :actor worker-slot
        :emits [ExecutionEvent::Completed])
      (transition :completed -> :completion_audited
        :actor autopilot-runtime
        :writes [mission_execution completion-note])
      (transition :completion_audited -> :done
        :actor autopilot-runtime
        :writes [BoardEvent::StatusChanged])
      :completion-rule "A delegated BoardTask is complete only after Autopilot observes worker completion or self-close, reconciles mission_execution completion, and emits the final BoardEvent status transition."))

  (policies
    (policy risk-gate
      :inputs [mode objective write_scope must_not_touch risk_level external_side_effects]
      :allow-auto-approval-if
        [:trusted_agent :low_or_medium_risk :bounded_write_scope
         :no_destructive_action :acceptance_present :rollback_or_blocker_present]
      :must-ask-human-if
        [:high_risk :ambiguous_goal :destructive_action :external_publish
         :payment_or_secret :unbounded_write_scope])

    (policy scoped-write-gate
      :inputs [plan.nodes.write_scope plan.nodes.must_not_touch git_status]
      :checks [:owned_paths_only :forbidden_paths_empty :nul_free :diff_check_clean])

    (policy parent-hotfix-finalization
      :rule "Parent patches after worker exit must append events and regenerate final-report lineage.")

    (policy verification-reuse
      :rule "Receipts may cover later states only when commit prefix, file set, tier, and exit_code rules pass."))

  (source-hygiene
    :desc "Read-only source and staged-index hygiene before scoped task commits."
    :entrypoints [scripts/check-staged-source-hygiene.mjs
                  scripts/task-scope-guard.mjs
                  .githooks/pre-commit]
    :hook-policy "Repo-local hook install is explicit opt-in; the pre-commit hook is a no-op unless MISSIOND_TASK_CONTRACT names a task.lisp contract."
    :invariants
      ["Staged hygiene MUST be read-only: no git add, commit, reset, checkout, stash, push, merge, rebase, hook mutation, or working-tree mutation."
       "MISSIOND_TASK_CONTRACT enables task-scope guard enforcement in the pre-commit hook; without it the hook exits 0 so non-task commits are not blocked."
       "Staged source hygiene MUST reject raw NUL bytes in staged blobs before commit."
       "Staged source hygiene MUST run git diff --cached --check over the staged path set."
       "Task-scope guard MUST reject staged paths outside :write-scope and any path matching :must-not-touch."
       "The hook doctor MUST be read-only by default; hook installation is a separate explicit install command."
       "Batch verification MAY import checkSuppliedFiles for final-tree source hygiene fixtures, but must not mutate git."])

  (multi-agent-context-pack
    :desc "Two-stage parallel investigation and shard implementation as a Lisp-owned append-only context bus."
    :schema "missiond.context-pack.v1"
    :write-model "multi-agent append-only"
    :entry-heads [claim observation anchor shard-proposal conflict integration-plan]
    :mutation-owner "append helper / writer-specific entry only; no worker rewrites prior entries"
    :merge-owner "orchestrator or context-integrator appends a single integration-plan after reading proposals"
    :flow [parallel-claims parallel-observations shard-proposals conflict-notes integration-plan compile-shards materialize-wave run-wave dispatch-code-workers verify-and-finalize]
    :roles
      ((context-investigator :writes [claim observation anchor shard-proposal conflict] :forbidden [code-edits commits])
       (context-integrator :writes [integration-plan] :reads [shard-proposal conflict])
       (code-worker :reads [integration-plan accepted-shards dispatch-groups] :writes [declared-shard-write-scope report commit])
       (parent-verifier :writes [verification-receipt final-report parent-patches]))
    :invariants
      ["Context investigators MAY run concurrently and append claim/observation/anchor/shard-proposal/conflict entries to the same context-pack.lisp."
       "Every entry MUST carry :id :agent :seq :at; :seq is strictly increasing and allocated by the append path, not guessed from stale reads."
       "shard-proposal entries MUST declare :shard :owner :write-scope :must-not-touch :acceptance so code workers can execute without re-deriving architecture."
       "integration-plan MUST cite accepted-shards and dispatch-groups; mapped dispatch groups SHOULD use (group :id <id> :shards [...]) so orchestration can compile code-worker waves without narrative parsing."
       "context-pack-materialize-wave MUST refuse names-only dispatch groups and may only project mapped integration-plan shards into task-runner manifest + task-contract files."
       "context-pack-run-wave is the single orchestration entry from context-pack SSOT to prepared task-runner wave and dispatch descriptor; it must not submit workers unless --apply is explicit."
       "context-pack-run-wave MUST create missing shared-memory/session-trace ledgers with create-only semantics before prepare-task-runner-wave, and MUST NOT rewrite existing ledgers."
       "Accepted shard write-scope entries MUST NOT overlap unless a later conflict entry explicitly routes that hotspot to a single owner."
       "Context pack writers produce evidence and proposals only; code implementation happens in later shard tasks with disjoint write scopes."
       "code workers consume the latest integration-plan through context-pack-compile-shards; they do not reinterpret investigator observations as authority."
       "Shared-memory remains coarse lifecycle memory; context-pack is the high-density planning surface that turns concurrent investigation into implementable shards."]
    :checker "node scripts/check-context-pack.mjs")
