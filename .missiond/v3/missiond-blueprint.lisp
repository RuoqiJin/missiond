;; MissionD v3 compact blueprint.
;; Goal: a high-density Lisp contract that MissionD can read as a runtime
;; constitution, while v1/v2 remain historical and implementation references.

(missiond-blueprint
  :schema "missiond.blueprint.v3"
  :version "v3.0-draft"
  :status "architecture-compressed"
  :authority "file-first-lisp"
  :root-flow unified-entry
  :legacy-v1 ".missiond/v1/manifest.lisp"
  :legacy-v2 ".missiond/v2/intent.lisp"

  (axioms
    (request-first
      :rule "Every external need enters as a mission_request before becoming work.")
    (artifact-first
      :rule "Reviewable truth is in Lisp artifacts; DB rows and markdown are projections.")
    (two-gate-default
      :rule "Human mode requires intent approval and plan approval before execution.")
    (trusted-agent-fast-path
      :rule "Trusted agents may fold intent approval into plan.lisp only through policy gates.")
    (orchestrator-final-truth
      :rule "Workers draft outcomes; orchestrator finalizes reports and completion state.")
    (event-sourced-runtime
      :rule "Runtime state is derived from append-only events, not manual ledger edits.")
    (lisp-code-isomorphism
      :rule "Code handlers implement named Lisp contracts; drift is a checker failure."))

  (artifact-contracts
    (artifact mission-request
      :schema "missiond.request.v1"
      :path ".missiond/requests/<request_id>/request.lisp"
      :ssot true
      :writer mission_request
      :required [:request_id :source :mode :objective :state :artifacts :policy]
      :states [:received :intent_drafted :awaiting_intent_approval
               :intent_approved :plan_drafted :awaiting_plan_approval
               :plan_approved :executing :verifying :done :blocked :failed])

    (artifact intent-alignment
      :schema "missiond.intent-alignment.v1"
      :path ".missiond/requests/<request_id>/intent-alignment.lisp"
      :compat-path ".missiond/alignment/<topic>/intent-alignment.lisp"
      :compat-status "legacy projection only — opt-in via mission_request compat_write_file=true (or legacy write_file=true alias). Default mission_request flow MUST NOT write the compat path; the V3 review surface is the request-local artifact."
      :ssot true
      :writer alignment-author
      :required [:request_id :objective :scope :assumptions :non_goals
                 :acceptance :risk :approval]
      :materialization-rule "When a persisted directive row is projected to Lisp, request-local and compatibility intent-alignment files MUST carry :directive_id + :version so a later approve_intent can advance by reading Lisp alone; callers must not need hidden DB ids."
      :review-gate intent-review-gate)

    (artifact plan
      :schema "missiond.plan.v1"
      :path ".missiond/requests/<request_id>/plan.lisp"
      :compat-path ".missiond/plans/<topic>/PLAN.lisp"
      :compat-status "legacy projection only — opt-in via mission_request compat_write_file=true (or legacy write_file=true alias). Default mission_request flow MUST NOT write the compat path; the V3 review surface is the request-local plan.lisp."
      :ssot true
      :writer plan-author
      :required [:request_id :intent :execution :nodes :gates :approval]
      :dry-run-scaffold
        (:required-hints [:target :objective :nodes]
         :default-target mission_task_delegate
         :rule "compiler_mode=dry_run must still emit executable routing hints in Lisp; plan-runner may derive target/objective from plan.lisp without caller args"
         :non-goal "dry_run does not bypass intent/plan review and does not dispatch before execute_plan")
      :materialization-rule "When approve_plan promotes request-local plan.lisp into a persisted Plan row, the request-local plan artifact MUST be amended with :plan_id + :version + :board_task_id so execute_plan can advance by reading plan.lisp alone; review events remain an append-only audit/fallback, not the primary ref carrier."
      :invariant "plan.intent preserves enough alignment data for trusted-agent audit")

    (artifact workflow
      :schema "missiond.workflow.v1"
      :path ".missiond/workflows/<topic>.lisp"
      :ssot true
      :writer workflow-distiller
      :required [:workflow_id :source_plans :match_rules :steps :status])

    (artifact lifecycle-event
      :schema "missiond.lifecycle-event.v1"
      :path ".missiond/requests/<request_id>/events/<seq>.event.lisp"
      :ssot true
      :writer append-event
      :required [:event_id :request_id :kind :actor :time :payload :idempotency_key]
      :invariant "one event per file; append API owns sequence allocation")

    (artifact context-pack
      :schema "missiond.context-pack.v1"
      :path ".missiond/tasks/<wave>/context-pack.lisp"
      :ssot true
      :writer context-pack-append
      :required [:schema :wave :purpose :write-model :sequence]
      :entries [claim observation anchor shard-proposal conflict integration-plan]
      :invariant "multi-agent append-only context ledger: every writer owns only its entry id/seq; integration-plan is a later projection over accepted-shards and dispatch-groups, not a rewrite of prior observations")

    (artifact final-report
      :schema "missiond.final-report.v1"
      :path ".missiond/requests/<request_id>/reports/final.lisp"
      :ssot true
      :writer finalizer
      :required [:request_id :report_state :final_commit_hash
                 :verification_receipts :parent_patches :outcome])

    (artifact verification-receipt
      :schema "missiond.verification-receipt.v1"
      :path ".missiond/requests/<request_id>/receipts/<receipt_id>.lisp"
      :ssot true
      :writer verifier
      :required [:receipt_id :valid_for_files :commit_hash :tier :exit_code :commands]))

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

  (workstation-config
    :desc "Lisp-owned workstation spawn policy; runtime slot config is a projection, not an independent default."
    :config-fields [:template :model_profile :model :cwd :project_root :mcp_config :ttl :permission_mode]
    :resolution-order [caller-model caller-model-profile task-intent-template claude-code-default]
    (model-profile coding-default-opus-4-7
      :applies-to [code research]
      :claude-code-ui "Default recommended"
      :effective-model "Opus 4.7 with 1M context"
      :spawn-model-arg nil
      :rule "Omit --model so Claude Code uses the user's Default model selection.")
    (model-profile research-default
      :applies-to [research review context-pack lisp-compression]
      :pool-binding gemini-ultra-pro
      :spawn-model-arg nil
      :rule "Research-class delegations route to the workstation-pool gemini researcher slot. spawn-model-arg is nil because the binding is to a pre-spawned Gemini PTY; explicit caller model_profile=coding-default-opus-4-7 overrides this and pins the work to Claude.")
    (model-profile gemini-ultra-pro-preview
      :applies-to [research review context-pack lisp-compression language-explanation]
      :pool-binding gemini-ultra-pro
      :spawn-model-arg "gemini-3.1-pro-preview"
      :rule "Gemini Ultra defaults to Gemini 3.1 Pro Preview for high-level read-only investigation; lower-authority fast survey lanes must be explicitly requested.")
    (model-profile codex-master-gpt-5-5-xhigh
      :applies-to [master-control orchestration governance night-audit]
      :pool-binding codex-master-control
      :spawn-model-arg "gpt-5.5"
      :reasoning-effort xhigh
      :search true
      :sandbox danger-full-access
      :approval-policy never
      :rule "Resident master control uses Codex GPT-5.5 with xhigh reasoning and full local sandbox access. It remains an audited orchestrator: every direct mutation must leave Board/KB/checkpoint evidence, while ordinary implementation still prefers delegated workers.")
    (model-profile daily-sonnet
      :applies-to [ops low-risk-maintenance]
      :spawn-model-arg "sonnet"
      :rule "Use only when the task or caller explicitly asks for Sonnet-class daily work.")
    (model-profile quick-haiku
      :applies-to [docs test chore low-risk-fast-path]
      :spawn-model-arg "haiku"
      :rule "Use only when the task or caller explicitly asks for a low-cost fast Claude Code model.")
    (slot-template coder
      :role coder
      :description "Dynamic coder slot (ephemeral)"
      :default-model-profile coding-default-opus-4-7
      :mcp-config "/Users/jinchen/.xjp-mission/xjp-mcp-config.json"
      :default-cwd "/Users/jinchen/Projects")
    (slot-template researcher
      :role coder
      :description "Dynamic researcher slot (read-only analysis)"
      :default-model-profile research-default
      :mcp-config "/Users/jinchen/.xjp-mission/xjp-mcp-config.json"
      :default-cwd "/Users/jinchen/Projects")
    (slot-template ops
      :role operator
      :description "Dynamic ops slot (ephemeral)"
      :default-model-profile daily-sonnet
      :mcp-config "/Users/jinchen/.xjp-mission/xjp-mcp-config.json"
      :default-cwd "/Users/jinchen/Projects")
    (cwd-policy dynamic-slot
      :allowed-prefixes ["/Users/jinchen/Projects" "/Users/jinchen/Documents" "/tmp"])
    (startup-slot arch_maintenance
      :engine claude-code
      :lifecycle persistent
      :slot_id "slot-arch-maint"
      :role arch-maint
      :model_profile coding-default-opus-4-7
      :timeout_secs 600
      :skip_permissions true)
    (startup-slot strategy_analyst
      :engine gemini
      :lifecycle persistent
      :slot_id "slot-gemini-strategy"
      :role strategy
      :model_profile nil
      :timeout_secs 600
      :skip_permissions true)
    (startup-slot gemini_router
      :engine gemini
      :lifecycle persistent
      :slot_id "slot-gemini-router"
      :role gemini-router
      :model_profile nil
      :timeout_secs 120
      :skip_permissions true)
    (startup-slot lisp_survey
      :engine claude-code
      :lifecycle persistent
      :slot_id "lisp-surveyor"
      :role coder
      :model_profile coding-default-opus-4-7
      :timeout_secs 900
      :skip_permissions true)
    (timeout-policy boardtask-dispatch
      :default_secs 1800
      :min_secs 60
      :max_secs 7200
      :watchdog_grace_secs 120
      :missing_session_probe_secs 120)
    (timeout-policy claudecode-swarm
      :default_secs 600
      :min_secs 60
      :max_secs 7200)
    (timeout-policy pty-send-blocking
      :default_secs 300
      :min_secs 1
      :max_secs 7200)
    (timeout-policy dynamic-slot-spawn
      :default_secs 60
      :min_secs 10
      :max_secs 600)
    (dispatch-policy context-pack-run-wave
      :default_max_parallel 4
      :min_parallel 1
      :max_parallel 8)
    (ttl-policy dynamic-slot
      :default_secs 14400
      :min_secs 300
      :max_secs 28800
      :default_extend_secs 3600
      :max_extend_secs 3600)
    :invariants
      ["code and research dynamic slots MUST NOT hardcode --model sonnet"
       "daemon startup SlotManager ClaudeCode task configs MUST project coder/researcher model profiles from workstation-config and omit --model for coding-default-opus-4-7"
       "daemon startup SlotManager task configs MUST be generated from workstation-config startup-slot entries, including engine/lifecycle/slot_id/role/timeout_secs/skip_permissions"
       "mission_compute_slot dynamic template role/description/mcp_config/default_cwd and allowed cwd prefixes MUST project from workstation-config slot-template + cwd-policy dynamic-slot, not a Rust-local template table"
       "model=\"default\" and model_profile=coding-default-opus-4-7 both mean no CLI --model override"
	       "mission_compute_slot model_profile resolution MUST use workstation-config model-profile spawn-model-arg, not a Rust-local profile table"
	       "caller-supplied model wins over model_profile, but must be a single shell token"
	       "task_delegate must pass model/model_profile through to compute_slot and must not reuse an idle slot with a conflicting model override"
	       "mission_task_delegate MUST accept structured two-stage delegation metadata (task_class, pool_hint, engine_hint, context_pack_path, read_scope, write_scope, must_not_touch, acceptance) and persist it into the BoardTask description so Autopilot workers see context-pack path, explicit readable evidence, exact write scope, forbidden write paths, and acceptance commands without relying on side-channel PTY text. The generated scope_semantics line MUST state that must_not_touch forbids write/stage/commit and is not a read ban by itself; review/context-pack/research tasks MUST carry an output_contract requiring a structured artifact with Findings / Evidence / Recommendations / Verification rather than raw KB JSON or full logs."
		       "mission_task_delegate MUST NOT auto-preload KB/Skill context from context_hints into worker prompts by default; current KB/skill stores are noisy and hidden prompt injection obscures task contracts. Context must come from explicit read_scope, context_pack_path, task contract, or a future explicit memory-audit workflow."
		       "Autopilot context prefetch defaults disabled until memory stores are cleaned: delegated worker prompts MUST NOT prepend KB/Skill/context-pipeline output unless an explicit memory-audit workflow opts in via MISSIOND_AUTOPILOT_CONTEXT_PREFETCH=1."
	       "mission_swarm_run MUST honor max_gemini_workers exactly: when max_gemini_workers=0, no spawned context-pack BoardTask may use intent=research or any other routing signal that sends the task to Gemini; Claude context-pack workers use code/coder routing plus read-only completion protocol."
	       "mission_swarm_run MUST resolve project_id to a registered project_root before creating external-project BoardTasks; generated swarm metadata and default read_scope must include that project_root so Autopilot can spawn provider PTYs in the target project instead of MissionD's own cwd. Worker-facing context_pack_path MUST be an absolute MissionD workspace path (or an already absolute caller path), because external-project workers run with cwd at the target project root and relative .missiond paths would point at the wrong project."
	       "Autopilot ensure_pty MUST override pty_slot.cwd to the BoardTask.project's registered project_root when the BoardTask carries a project label that resolves under ProjectRegistry and that root differs from slot.config.cwd; spawn_tracked_slot's project-root-spawn-cwd contract then handles Gemini/Codex hard-fail and ClaudeCode normalization. Slot reuse for cross-project dispatch MUST require slot.project_root == BoardTask project_root (already enforced for mission_task_delegate; mission_swarm_run BoardTasks rely on the spawn-side cwd override for the same effect)."
	       "Autopilot/flow-engine BoardTask dispatch MUST bind conversations.task_id to the active BoardTask via a bounded retry helper at dispatch time (5 attempts at 200 ms) and MUST re-bind after the worker final settle window to cover provider JSONL/session-discovery races; completion-time durable_provider_completion_for_slot_task remains a fallback. Because one provider session may execute multiple sequential BoardTasks, mission_conversation_query(taskId=...) MUST also recover provider conversations whose durable messages contain the BoardTask id, so a later task rebind cannot make an earlier task's audit log disappear."
	       "Autopilot MUST treat explicit engine_hint/pool_hint as hard constraints when the V3 workstation-pool declares at least one matching worker: if that worker is busy or stopped, the task waits instead of silently spending a different provider. Fallback to a non-matching worker is allowed only when no declared worker satisfies the hint at all, and that fallback MUST record a durable reroute_reason as a BoardTask note before dispatching so the operator can see why the requested engine/pool was not used."
	       "mission_task_delegate intent=research without an explicit Claude coding model/model_profile MUST prefer the workstation-pool gemini researcher slot (slot-gemini-ultra) when registered; the researcher slot-template's :default-model-profile is research-default, which binds to the gemini-ultra worker. Auto-provisioning a dynamic Claude slot for research is forbidden while a V3 gemini researcher slot exists; the BoardTask is queued unassigned and the autopilot routes it to the gemini slot once idle. Explicit model_profile=coding-default-opus-4-7 (or any Claude profile) still routes the BoardTask to Claude."
       "Project-bound workstation spawn MUST sync MissionD Claude hooks into <project>/.claude/settings.local.json before PTY start and MUST inject MISSION_IPC_ENDPOINT into the slot env; this preserves global ~/.claude/settings.json while making SessionStart UUID capture and UserPromptSubmit context prefetch local, idempotent, and project-scoped"
       "Autopilot pty.send budget MUST project from BoardTask.timeout_secs (default 1800s, clamped 60..7200) — never a fixed 600_000ms — so a delegated long-running task gets the timeout the delegator already declared"
       "mission_cc_swarm pty.send budget MUST project from workstation-config timeout-policy claudecode-swarm (default 600s, clamped 60..7200) — never a local 600_000ms literal"
       "mission_pty_send waitForResponse budget MUST project from workstation-config timeout-policy pty-send-blocking (default 300s, clamped 1..7200) — never a local 300_000ms literal"
       "mission_compute_slot and Claude/Gemini slot-orchestrator dynamic slot spawn wait_for_idle timeouts MUST project from workstation-config timeout-policy dynamic-slot-spawn (default 60s, clamped 10..600) — never local Some(60)/Some(120) literals"
       "context-pack-run-wave default worker fanout MUST project from workstation-config dispatch-policy context-pack-run-wave (default 4, clamped 1..8), while caller --max-parallel remains an explicit override"
       "Dynamic slot TTL and per-request extension budget MUST project from workstation-config ttl-policy dynamic-slot (create default 14400s, clamped 300..28800; extend default/max 3600s) — direct mission_compute_slot create/extend and delegated task_delegate auto-provision must not hardcode the TTL window"
       "Smart watchdog idle-recovery threshold MUST equal the projected pty.send budget plus a small grace (default 120s); only the no-PTY-session branch may reclaim sooner so a missing process can never wedge the slot"
       "Autopilot BoardTask claim lease MUST equal the smart-watchdog idle-recovery threshold (projected pty.send budget plus grace); the legacy fixed 20-minute lease is forbidden because it lets the watchdog reclaim a slot whose claim is still legitimately ticking inside the declared timeout"
       "Autopilot summary-note source MUST prefer durable provider final evidence after wait_for_worker_final_settle_window(), per-session reconcile, and a bounded await_durable_provider_completion_for_slot_task poll. Durable assistant messages that are tool invocations, intermediate investigation narration such as 'Let me inspect/check/read...', or mutation-progress narration such as 'Now committing...' are not valid finals. Raw res.response is forbidden in the **Autopilot 执行完成** note format string and in the synthesized mission_execution(action=complete) summary; fallback extract_worker_final_summary(res.response, full_prompt) is allowed only after durable evidence is unavailable and MUST strip bare tool-call lines such as Bash(...), ●/⎿ tool logs, echoed task contract, and `[Pasted text +N lines, paste again to expand]` collapse markers. Auth-error and quota-exhausted diagnostic notes intentionally bypass this path and keep the raw response so on-call sees the verbatim platform error"
       "Autopilot close path MUST call wait_for_worker_final_settle_window() after pty.send completion and before summary-note / mission_execution / BoardTask done writes, then poll durable provider evidence for one additional bounded settle budget before using the PTY fallback. PTY idle alone is diagnostic, while pty.send completion plus durable evidence or sanitized fallback is the high-confidence final summary path."
       "If pty.send returns an active/progress frame but the provider later records a durable final assistant message in claude-jsonl/codex-sqlite/gemini-chat-file, or the master/worker later writes a durable BoardTask summary note, and the claimed slot is idle, Autopilot watchdog MUST synthesize the missing BoardTask summary when needed and close the running BoardTask from durable provider-or-note evidence + idle slot diagnosis; it MUST NOT wait for the full timeout/grace or close from PTY idle alone."]
    (prompt-tool-contract autopilot-claudecode-prompt
      :applies-to [coder researcher ops]
      :always-shown
        ["Board Task ID is surfaced in every dispatched prompt so the worker (and any reader of the prompt snapshot) can audit which BoardTask the slot is executing."]
      :objective-dedupe
        "Prompt assembly MUST suppress duplicated objective text: when BoardTask.description equals BoardTask.title or starts with the title followed only by blank lines, the assembled prompt uses description alone; distinct title + description still renders both joined by a blank line."
      :board-self-close
        (:mode conditional
         :when-tools-present "If mission_board_update and mission_board_note_add MCP tools are attached to the slot, the worker SHOULD call mission_board_update(status=\"done\") and mission_board_note_add(noteType=\"summary\") when the task completes."
         :when-tools-absent "If those board tools are not attached to the slot, the worker MUST instead return a concise final completion summary; Autopilot/orchestrator remains responsible for closing the BoardTask."
         :rationale "ClaudeCode slots may run with reduced MCP surfaces; an unconditional must-call instruction makes such slots unable to honor the prompt and leaks orchestrator state into the worker contract.")
      :non-prompt-guidance
        ["Decision Engine escalation suffix (mission_question_create) and ops-task focus suffix remain behaviorally intact and are appended after the deduplicated base prompt."])
    (execution-ownership delegated-boardtask
      :applies-to [coder researcher ops]
      :prompt-owner
        "For delegated BoardTask execution, Autopilot is the sole task-prompt owner. mission_task_delegate auto-provision (compute_slot/spawner) MAY warm a dynamic slot but MUST NOT send the task objective as a fire-and-forget initial-prompt; objective is slot metadata only. The slot starts idle and Autopilot sends the BoardTask prompt via state.pty.send. Direct compute_slot warmup requires an explicit initial_prompt field."
      :close-owner
        (:default "Autopilot is the close owner — when state.pty.send returns Complete, Autopilot transitions the BoardTask running→done and writes the summary note."
         :worker-self-close "If the slot has board MCP tools attached and the worker already drove the BoardTask to Done via mission_board_update before pty.send returns, Autopilot preserves the worker's Done state and only logs that the task self-closed."
         :execution-log-synthesis "If the delegated prompt carries a pre-opened mission_execution log and the slot returned a final summary without appending a completion, Autopilot MUST synthesize mission_execution(action=complete, commit_status=\"not-required\", enforce_scoped_commit=true) before closing the BoardTask."
	         :summary-note-source "The `**Autopilot 执行完成**` BoardTask summary note and the synthesized mission_execution(action=complete) summary MUST prefer durable provider final evidence after settle + per-session reconcile + bounded durable-final polling (claude-jsonl/codex-sqlite/gemini-chat-file), but durable assistant tool-invocation records such as `[Tool: Bash] ...`, active/progress frames, intermediate investigation narration such as 'Let me inspect/check/read...', and mutation-progress narration such as 'Now committing...' are not valid finals; only fall back to extract_worker_final_summary(res.response, full_prompt) when no valid durable final exists after polling. The note body is capped via truncate_safe, and passing raw res.response into the note format string is forbidden because the Claude Code TUI screen capture includes the echoed prompt + task contract, bare Bash(...)-style tool-call lines, ●/⎿ tool log lines, and `[Pasted text +N lines, paste again to expand]` collapse markers. Auth-error and quota-exhausted diagnostic notes intentionally bypass this path and keep the raw response so on-call operators see the verbatim platform error."
	         :settle-window "After pty.send returns Complete, Autopilot MUST wait through wait_for_worker_final_settle_window() and then poll durable provider evidence for one bounded settle budget before writing the summary note, synthesizing mission_execution completion, or transitioning the BoardTask to Done; default settle is intentionally long enough for provider JSONL/SSE final text to land, and may be overridden only by MISSIOND_AUTOPILOT_FINAL_SETTLE_MS."
	         :idle-durable-summary-close "If pty.send returned an active/progress frame and left the BoardTask running, a later watchdog tick MAY close only when the claimed slot is Idle AND either get_board_task_with_notes shows a claim-after durable summary note OR the provider conversation store has a claim-after task prompt plus assistant final for the same BoardTask. Provider-final closure must synthesize a BoardTask summary note and backfill conversation.task_id before Done; this closes from durable evidence plus idle diagnosis, never from PTY idle alone."
	         :blocked "If the task transitioned to Blocked (e.g. mission_question_create) during execution, Autopilot preserves the Blocked state on pty.send return and never overwrites it with done.")
      :dispatch-guard
        "The per-slot dispatch guard MUST be held across the entire state.pty.send call; the legacy release-before-send pattern allowed a second caller to dispatch to the same slot mid-flight. The guard is per-slot, so holding it does not starve callers targeting other slots."
      :concurrent-slot-dispatch
        "Autopilot dispatch_board_tasks MUST start state.pty.send work concurrently across different slots within a single dispatch tick; the legacy serial loop awaited one slot's pty.send before any other slot's send could begin, which starved every other ready slot for the duration of one slot's send. The implementation MUST hand each ready BoardTask's send + post-send tail to a tokio::task::JoinSet task with an OwnedSlotDispatchGuard moved in, so different-slot sends start in the same tick while same-slot exclusion still covers the entire send + close-owner / KB-feedback / deploy-review sequence. The outer dispatch_board_tasks MUST drain the JoinSet via join_next so quota / global-pause / KB-feedback / retry semantics still complete before the dispatch tick returns."
      :restart-recovery
        "Restart recovery MUST clear stale slot-dyn-* BoardTask assignee pins when the runtime slot is absent and the dynamic_slots row is not active, using BoardStore::clear_board_task_assignee before normal no-assignee routing resumes."
	    :rationale
	        "Wave33 evidence: a delegated BoardTask was sent twice — once via spawner.initial_prompt fire-and-forget, then again via Autopilot pty.send — and the slot's TextOutputEvent::Complete arrived without Autopilot transitioning the BoardTask to done. Single ownership of prompt+close eliminates the orphaned-task class entirely."))

  (workstation-pool
    :desc "Compact V3 SSOT for human-owned external compute accounts exposed as MissionD workers."
    :account-mode single-login
    :selection [task_class capability idle_state same_slot_guard]
    :evidence ".missiond/v3/evidence/workstation-pool.lisp"
    (worker claude-code-default
      :engine claude-code
      :role coder
      :slot-id "slot-claude-code-default"
      :task-type claude_code_default
      :model-profile coding-default-opus-4-7
      :model nil
      :task-classes [code implementation review context-pack ops]
      :capabilities [code-read code-write scoped-commit mcp]
      :max-concurrency 1
      :timeout-secs 1800
      :default-use code-implementation
      :accepts-boardtask true
      :write-allowed true)
    (worker claude-code-fast-patch
      :engine claude-code
      :role patcher
      :slot-id "slot-claude-code-fast-patch"
      :task-type claude_code_fast_patch
      :model-profile daily-sonnet
      :model nil
      :task-classes [patch test chore low-risk-fast-path]
      :capabilities [code-read code-write scoped-commit narrow-patch mcp]
      :max-concurrency 1
      :timeout-secs 900
      :default-use narrow-patch
      :accepts-boardtask true
      :write-allowed true)
    (worker gemini-ultra-pro
      :engine gemini
      :role researcher
      :slot-id "slot-gemini-ultra"
      :task-type gemini_ultra
      :model-profile gemini-ultra-pro-preview
      :model nil
      :approval-policy plan
      :tool-policy-path ".missiond/v3/policies/gemini-readonly-policy.toml"
      :task-classes [research review context-pack lisp-compression general]
      :capabilities [read-only analysis design-review]
      :max-concurrency 1
      :timeout-secs 900
      :default-use research-review
      :accepts-boardtask true
      :write-allowed false)
    (worker gemini-fast-survey
      :engine gemini
      :role survey
      :slot-id "slot-gemini-fast-survey"
      :task-type gemini_fast_survey
      :model-profile nil
      :model "gemini-2.5-flash"
      :approval-policy plan
      :tool-policy-path ".missiond/v3/policies/gemini-readonly-policy.toml"
      :task-classes [survey summary mechanical-scan]
      :capabilities [read-only summary]
      :max-concurrency 1
      :timeout-secs 600
      :default-use low-authority-survey
      :accepts-boardtask true
      :write-allowed false)
    (worker codex-master-control
      :engine codex
      :role orchestrator
      :slot-id "slot-codex-master-control"
      :task-type codex_master_control
      :model-profile codex-master-gpt-5-5-xhigh
      :model nil
      :reasoning-effort xhigh
      :search true
      :sandbox danger-full-access
      :approval-policy never
      :task-classes [master-control orchestration governance night-audit]
      :capabilities [board-write kb-write execution-log dispatch code-read code-write shell-exec search mcp full-access]
      :max-concurrency 1
      :timeout-secs 7200
      :default-use resident-master-control
      :accepts-boardtask false
      :write-allowed true)
    :invariants
      ["Claude coding workers use coding-default-opus-4-7, which means no Claude Code --model override; Sonnet cannot be the coding default."
       "Codex master-control is a resident orchestrator lane with full local sandbox access, not a normal BoardTask code shard candidate; it may directly repair MissionD control-plane issues when necessary, but every direct mutation must leave Board/KB/checkpoint evidence and ordinary implementation still prefers delegated workers."
       "Claude fast-patch may use Sonnet only for narrow atomic tasks whose context-pack already identifies exact files/regions; it is not a default coding lane."
       "Gemini Ultra Pro is the high-language read-only investigation lane using gemini-3.1-pro-preview; Gemini fast survey is explicitly low-authority mechanical scan/summary work."
       "Gemini is initially read-only: research, review, context-pack, and Lisp compression advice may route there; scoped write/commit work stays on Claude until a separate Gemini write smoke passes."
       "Read-only Gemini pool workers MUST project to Gemini CLI `--approval-mode plan --policy .missiond/v3/policies/gemini-readonly-policy.toml`; workstation-pool registration MUST NOT set dangerously_skip_permissions/YOLO for any worker with :write-allowed false, and the policy MUST deny subagent delegation and write/shell tools."
       "Autopilot unassigned BoardTasks select from workstation-pool by task class before considering any legacy slot; old slots.yaml Sonnet entries are not generic coding candidates."
       "mission_compute_slot action=list must expose workstation_pool with runtime slot presence and idle/busy/stopped status."
       "Supervisor patrol (slot-supervisor) is gated on V3 workstation-pool / runtime-config registration; absent a supervisor worker entry the patrol stays inert and MUST NOT call ensure_memory_slot_by_id, so the legacy 'Memory slot not configured in slots.yaml' warning cannot fire."
       "V3 workstation-pool (plus startup-slots) is authoritative for dispatchable slots; mission_compute_slot list MUST tag any static slot whose id is not in the V3 projection as legacy=true and dispatchable=false (or split it into legacy_static_slots) so retired Sonnet entries (autopilot/topology-guardian/extraction-worker/delta-validator/...) cannot resurface as candidates."
       "mission_compute_slot list status MUST derive from PTYManager (state.pty.get_status) for every slot it surfaces, so it cannot contradict mission_pty_status for V3 pool slots; the SlotManager session_id field is only a fallback when no PTY status exists, and it MUST NOT report 'running' when no PTY is attached."]
    :checker "node scripts/check-v3-workstation-pool-isomorphism.mjs")

  (resident-master-control
    :desc "Resident Codex brain layer: event-driven orchestrator that reads Lisp SSOT, Board, KB, project registry, execution logs, and worker telemetry, then delegates exact work to pool workers."
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
	       :fields [active_objective_id phase context_pack_path delegated_task_ids blocked_reason last_verified_commit resume_instruction])
    :event-subscriptions
      [BoardTaskCreated BoardTaskStatusChanged SlotEvent QuestionEvent SystemEvent::ContextualCommitDetected DaemonRestart StaleTask NightSchedule ProjectRegistryChanged]
    :loop
      ((step s1 :logic "load latest checkpoint, active Board objectives, V3/project Lisp registries, and recent event tail")
       (step s2 :logic "classify events into decision-required, dispatchable, blocked, stale, or informational")
       (step s3 :logic "for dispatchable work, emit context-pack organizer tasks first, then exact write-scope shards")
       (step s4 :logic "delegate Claude/Gemini/Codex workers through BoardTask/Autopilot only; never bypass durable event/Board state")
       (step s5 :logic "write checkpoint + Board note + execution companion log after every decision boundary"))
    :evidence-authority
      ((tier t1 :source [provider-jsonl codex-sqlite claude-jsonl gemini-chat-file] :use "durable final/progress facts")
       (tier t2 :source [missiond-event-bus BoardTask-lifecycle mission_execution] :use "causal workflow state")
       (tier t3 :source [provider-aware-pty-recognition screen-buffer] :use "diagnostic state only; never sole completion authority"))
    :settle-policy
      "A worker can be closed only after durable final event or high-confidence final summary plus settle window; idle PTY alone is insufficient because provider SSE/final JSONL can lag the prompt returning."
    (master-checkpoint
      :entry [daemon-startup event-wakeup periodic-heartbeat daemon-restart-before-exit]
      :core
        ((step s1 :logic "read latest event cursor, queue counters, blocked reason, MCP readiness, and current objective")
         (step s2 :logic "record daemon restart/startup context for checkpoint visibility without calling notify or incrementing queued control events")
         (step s3 :logic "resolve checkpoint root from the V3-projected master slot project_root/cwd; never infer it from daemon process current_dir")
         (step s4 :logic "render master-control-checkpoint.lisp under .missiond/v3/runtime; last-control-prompt is nil for heartbeat/startup ticks and present only when a control turn is dispatchable")
	         (step s5 :logic "store active_objective_id, phase, context_pack_path, delegated_task_ids, blocked_reason, last_verified_commit, and resume_instruction"))
	      :egress [".missiond/v3/runtime/master-control-checkpoint.lisp" "mission_master_status.checkpoint" "mission_convergence_status.runtime_status.checkpoint"]
      :surfaces ["crates/missiond-daemon/src/engine/master_control.rs::write_startup_checkpoint_for_slot"
                 "crates/missiond-daemon/src/engine/master_control.rs::render_checkpoint"])
    (master-event-subscriber
      :entry [BoardEvent SlotEvent QuestionEvent ProjectRegistryChanged DaemonRestart StaleTask NightSchedule]
      :core
        ((step s1 :logic "subscribe to BoardEvent, SlotEvent, and QuestionEvent with live-only v2 subscription names, StartFrom::Latest, and PerEvent cursor flush so daemon restart does not replay historical backlog")
         (step s2 :logic "ignore slot-codex-master-control self SlotEvents so the resident brain cannot trigger an infinite self-prompt loop")
         (step s3 :logic "ignore seq=0 volatile events, ordinary SlotEvent.became_idle noise, and SlotEvent.task_dispatched worker lifecycle noise; PTY idle/task-dispatched is diagnostic evidence, not a master-control wakeup authority")
         (step s4 :logic "filter worker creation/running noise before the model while preserving terminal worker edges: swarm worker TaskCreated and dev/running updates do not wake the resident master, but status_changed/updated done/completed/closed/failed/blocked MUST wake it so parent objectives can advance from durable worker completion")
         (step s5 :logic "filter swarm-created worker BoardTasks such as Investigate context for swarm objective, Survey exact shards for swarm objective, and Implement accepted swarm shard, because those terminal worker units belong to Autopilot/provider evidence rather than recursive master delegation")
         (step s6 :logic "same-process Board tool handlers also call notify_board_event_direct immediately after durable DB mutation and before/alongside event-log publish, so master wakeup is not blocked behind dispatcher backlog; Board notes authored by codex-master-control MUST NOT direct-notify the master again")
         (step s7 :logic "record only wakeup metadata and ack immediately")
         (step s8 :logic "notify master-decision-loop; never run long worker dispatch inline"))
      :egress [master-control-runtime.event-cursor master-control-runtime.queued-events master-control-runtime.notify]
      :surfaces ["crates/missiond-daemon/src/engine/master_control.rs::spawn_master_event_subscriber"])
    (master-decision-loop
      :entry [master-control-runtime.notify periodic-heartbeat]
      :core
        ((step s1 :logic "probe codex MCP server readiness from codex mcp list and unattended approval readiness from ~/.codex/config.toml tool approval_mode entries")
         (step s2 :logic "classify phase as observe_event -> classify_objective -> create_context_pack -> dispatch_investigators -> compile_shards -> dispatch_implementers -> verify -> close_or_backfill, then materialize context_pack_path as master-control-context-pack.v1 before prompting so the agent does not need to remember file creation")
         (step s2b :logic "build an architectural SSOT review prompt that asks what is logically inconsistent, missing, or optimizable from MissionD V3 SSOT Lisp, V3 checker/final-convergence static results, and recent .missiond/v3/** commits only; Board backlog, KB, event-history, provider durable logs, worker telemetry, and historical conversations are excluded from default self-review until explicit cleanup workflows opt in")
         (step s2c :logic "active_objective_id overrides default self-review: first query exactly that BoardTask by id, follow its description as the load-bearing objective, and read only the project roots/files explicitly named by that BoardTask or its context_pack_path; do not browse Board open backlog; default exclusions still block Board backlog, KB, event-history, provider logs, and historical conversations unless the active task explicitly opts them in")
         (step s2c2 :logic "require MissionD MCP narrowly: prefer mission_intent(project=missiond, action=summary) and mission_convergence_status for default self-review; active objectives may use only the MCP surfaces needed for that BoardTask. do not call mission_kb_query, mission_conversation_query, or provider-log tools during default periodic self-review. A decision of create/update BoardTask or close_or_backfill MUST execute the matching MissionD MCP mutation such as mission_board_note_add, mission_board_update, or mission_board_create before the resident slot returns its final decision; if mutation is unavailable, return blocked")
         (step s2d :logic "mission_convergence_status is a heavyweight diagnostic surface: successful live static snapshots are cached under .missiond/v3/runtime/convergence-status-cache.json, and live timeout returns cached_after_timeout with a warning instead of converting a recent cached OK snapshot into a false blocking item")
         (step s3 :logic "write checkpoint before any durable Board/KB/dispatch action")
         (step s4 :logic "on daemon-startup, ensure slot-codex-master-control is spawned when Exited/Error but do not consume a control turn; startup is for residency, not decision work")
         (step s5 :logic "before sending an event control turn, ensure slot-codex-master-control is spawned when Exited/Error, wait up to 180s for Idle/SlashMenu because gpt-5.5 xhigh control turns are brain-lane work rather than narrow-patch work, and verify the visible Codex footer still matches gpt-5.5 xhigh; if the slot was downgraded by an interactive model/rate-limit prompt, restart it before dispatch")
         (step s6 :logic "send control turns to slot-codex-master-control only on event-wakeup or pending periodic-heartbeat retry, with MCP server ready, required MCP tool approvals ready, and rate-limit guard")
         (step s6b :logic "when mission_control pauses the strategy domain or orchestrator slot_role, master-control MUST only write a paused checkpoint and MUST NOT auto-start the resident slot, send a control turn, run nightly evolution, or create self-evolution tasks; this lets operators supervise long-running repair waves without heartbeat restarts breaking provider sessions")
         (step s7 :logic "if a queued objective cannot receive a control turn because Codex master is still starting or the Codex MCP probe is transiently not ready, keep the queued event and retry from periodic-heartbeat once MCP and the slot are ready; successful control turn drains the queued event batch")
         (step s7b :logic "while an active objective exists, periodic-heartbeat MAY run a lightweight control turn no more often than every 900s; event wakeups remain immediate, but periodic self-review must not spam the resident master while KB/Board/provider evidence is noisy")
         (step s7c :logic "classification MUST preserve the current active_objective_id across worker SlotEvents or child BoardTask status events; an event with no top-level task_id must never clear the parent objective")
         (step s7d :logic "when the active parent objective itself emits a terminal BoardEvent.status_changed edge such as Running->Done/Running->done/Failed/Blocked, detect it case-insensitively, clear active_objective_id, consume the queued event without sending a Codex control turn, and stop periodic heartbeat from reprocessing a completed objective; terminal Board status events must also never create a new active objective during daemon-startup recovery")
         (step s8 :logic "detect code-first diffs and create a deduped backfill BoardTask instead of silently accepting Lisp/code drift")
         (step s9 :logic "defer long work to BoardTask/Autopilot; default periodic self-review must not inspect provider durable logs until an explicit worker-completion or memory-audit workflow opts in"))
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
         (step s3 :logic "reconcile Board open tasks against provider JSONL/Codex sqlite/Gemini chat files")
         (step s4 :logic "resume or requeue only from durable evidence; PTY recognition is diagnostic")
         (step s5 :logic "never auto-hide, skip, delete, or mutate historical Board cleanup candidates; legacy ops cleanup remains user-directed"))
      :egress [mission_master_status.service BoardTask-note])
    (night-scheduler
      :entry [schedule-metadata manual-objective mission_nightly_evolution]
      :policy (:workflow ".missiond/workflows/nightly-evolution.lisp"
               :schedule-window "daily"
               :default-mode observe-only
               :budget-secs 7200
               :max-followup-tasks 3
               :risk-gate "apply=true selects only findings whose class matches the requested mode; safe-backfill requires low risk, needs-investigation creates read-only context work, and proposal/user-decision modes create proposal tasks only.")
      :core
        ((step s1 :logic "NightlyEvolutionService reads only MissionD V3 blueprint, V3 checker output, final convergence static snapshot, and recent commits touching .missiond/v3/**; default nightly mode excludes KB, historical conversations, provider durable logs, worker telemetry, and Board open tasks")
         (step s2 :logic "write observe-first .missiond/v3/runtime/nightly-evolution/<date>.report.lisp")
         (step s3 :logic "materialize visible proposal/backfill BoardTasks only when apply=true, requested mode matches finding class, and risk gate allows it")
         (step s4 :logic "prefer read-only MissionD V3 SSOT investigation and context-pack generation")
         (step s5 :logic "checkpoint before and after each batch so daemon restart can resume"))
      :egress [nightly-evolution-report BoardTaskCreated master-control-checkpoint mission_master_status.nightlyEvolution])
    (commit-lisp-convergence-loop
      :entry [SystemEvent::ContextualCommitDetected mission_execution.complete provider-durable-log]
      :core
        ((step s1 :logic "CommitConvergenceService subscribes to SystemEvent::ContextualCommitDetected with StartFrom::Latest and PerEvent cursor flush")
         (step s2 :logic "resolve project from slot project_root/cwd or project registry; unknown project writes a diagnostic report only")
         (step s3 :logic "read committed snapshot with git diff-tree --root --no-commit-id -r --name-only <sha>, never current worktree diff")
         (step s4 :logic "classify changed files into code, lisp, checker, evidence, docs, or other")
         (step s5 :logic "for code-only commits create one visible deduped BoardTask commit-lisp-backfill:<project>:<sha>; lisp/checker/evidence-only commits do not recurse")
         (step s6 :logic "write .missiond/v3/runtime/commit-lisp-convergence/<sha>.report.lisp and expose commitConvergence status"))
      :egress [commit-convergence-report backfill-boardtask mission_master_status.commitConvergence])
    (nightly-evolution-loop
      :entry [night-scheduler mission_nightly_evolution final-convergence-snapshot]
      :core
        ((step s1 :logic "collect evidence only from MissionD V3 blueprint, V3 checker output, final convergence static snapshot, and recent commits touching .missiond/v3/**")
         (step s2 :logic "detect MissionD V3 SSOT issues: contradictory loops, structure repetition, surface/checker gaps, runtime projection gaps, missing entry/core/egress steps, and repeated Lisp prose")
         (step s3 :logic "classify findings as observe-only, safe-backfill, needs-investigation, architecture-proposal, or requires-user-decision")
         (step s4 :logic "default observe-only writes report; apply=true selects a finding by requested mode and may create one visible follow-up BoardTask")
         (step s5 :logic "master supervises anomalies; routine safe backfill can later be delegated directly through workflow trigger"))
      :egress [nightly-evolution-report proposal-boardtask master-control-checkpoint])
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
    :default-manifest "/Users/jinchen/Projects/universe.intent.lisp"
    :allowed-root "/Users/jinchen/Projects"
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
    :anthropic-ops-model "claude-sonnet-4-6"
    :anthropic-docs-test-chore-model "claude-haiku-4-5-20251001"
    :compress-model "gemini-3.1-pro"
    :compress-channel "google"
    :compress-max-tokens 2048
    :compress-char-budget-chars 100000
    :direct-http-timeout-secs 60
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
       "mission_router_chat default model and max_tokens MUST project from router-runtime-policy; explicit caller model/max_tokens still wins."
       "mission_router_chat_manage history lookup and compression model/channel/token/char budgets MUST project from router-runtime-policy."
       "Flow daemon Gemini calls, stateless Sonnet calls, and queued SonnetGateway calls MUST project their model and direct HTTP timeout from router-runtime-policy."
       "GeminiPtyDriver default slot model MUST project from router-runtime-policy flow-gemini-model; explicit caller model still wins."
       "Gemini CLI transport missing llm.yaml model MUST project from router-runtime-policy flow-gemini-model; explicit llm.yaml gemini_cli.model still wins."
       "GeminiClient CLI mode MUST forward non-empty caller model to GeminiCli and use the V3-projected GeminiCli default only when the caller omits model."
       "GeminiClient PTY/HTTP request queue timeouts MUST project from router-runtime-policy, preserving PTY starvation protection without local 30s/300s literals."
       "Gemini File API upload and poll timeouts MUST project from router-runtime-policy instead of local 600s/300s literals."
       "Gemini CLI absolute and tool-exec timeouts MUST project from router-runtime-policy instead of local 900s/300s literals."
       "Queued SonnetGateway quota throttle sleep MUST project from router-runtime-policy instead of a local 30s literal."
       "Translation worker message_translations.model MUST record the queued SonnetGateway model projected from router-runtime-policy instead of a local MiniMax literal."
       "xjp-router embedding client MUST project its missing timeout default from router-runtime-policy direct HTTP timeout; explicit llm.yaml timeout_secs still wins."
       "BoardTask urgent/ops/docs-test-chore ANTHROPIC_MODEL overrides MUST project from router-runtime-policy, not Rust literals."])

  (project-registry-policy
    :desc "Lisp-owned project registry defaults for intent discovery and universe import."
    :intent-path-candidates [".missiond/intent.lisp" ".jarvis/intent.lisp" "intent.lisp"]
    :default-universe-manifest "/Users/jinchen/Projects/universe.intent.lisp"
    :env-overrides [UNIVERSE_MANIFEST]
    :invariants
      ["mission_project init/import_universe/survey MUST project intent-path candidates from project-registry-policy."
       "mission_project import_universe MUST project its default manifest from project-registry-policy; UNIVERSE_MANIFEST is only an explicit override."
       "A real MissionD project with .missiond but no project-registry-policy MUST return V3_BLUEPRINT_CONFIG_ERROR rather than silently using embedded defaults."])

  (project-blueprint-registry
    :schema "missiond.project-blueprint-registry.v1"
    :rule "Project-local app blueprints are independent SSOT files registered from V3; backend V3 stays compact and aggregate checkers follow the registry pointer."
    (project :id board
      :kind frontend-nextjs
      :path ".missiond/frontend/board-blueprint.lisp"
      :package "packages/board/package.json"
      :status code-aligned
      :checks ["node scripts/check-frontend-board-lisp-schema.mjs"
               "node scripts/check-frontend-board-code-isomorphism.mjs"
               "node scripts/check-frontend-board-runtime-projection.mjs"
               "node scripts/project-frontend-board-config.mjs --check"]
      :surface board-frontend)
    (project :id jarvis-forge
      :kind multi-crate-nextjs
      :root "/Users/jinchen/Projects/jarvis-forge"
      :intent ".missiond/intent.lisp"
      :backend ".missiond/backend/forge-backend-blueprint.lisp"
      :frontend ".missiond/frontend/forge-ui-blueprint.lisp"
      :status project-ssot-owned
      :missiond-role "registered project; Lisp/component reuse engine, not MissionD runtime orchestrator"
      :surface project-registry)
    (project :id xiaojinpro-backend
      :kind rust-monorepo
      :root "/Users/jinchen/Projects/xiaojinpro-backend"
      :intent ".missiond/intent.lisp"
      :backend ".missiond/backend/xiaojinpro-backend-blueprint.lisp"
      :status ssot-seeded
      :checks ["node scripts/check-xjp-ssot-complete.mjs"]
      :surface project-registry)
	    (project :id deploy-center
	      :kind ops-service
	      :root "/Users/jinchen/Projects/xiaojinpro-backend/services/deploy-center"
	      :intent ".missiond/intent.lisp"
	      :backend ".missiond/backend/deploy-center-backend-blueprint.lisp"
	      :status universe-imported
	      :capability deploy-ops
	      :surface project-registry)
    (project :id deploy-agent
      :kind ops-agent
      :root "/Users/jinchen/Projects/xiaojinpro-backend/crates/xjp-cli"
      :intent ".missiond/intent.lisp"
      :backend ".missiond/backend/deploy-agent-backend-blueprint.lisp"
      :status ssot-seeded
      :capability deploy-ops
      :surface project-registry)
    (project :id auth
      :kind rust-service
      :root "/Users/jinchen/Projects/xiaojinpro-backend/services/auth"
      :intent ".missiond/intent.lisp"
      :backend ".missiond/backend/auth-backend-blueprint.lisp"
      :status ssot-seeded
      :surface project-registry)
    (project :id router
      :kind rust-service
      :root "/Users/jinchen/Projects/xiaojinpro-backend/services/router"
      :intent ".missiond/intent.lisp"
      :backend ".missiond/backend/router-backend-blueprint.lisp"
      :status ssot-seeded
      :surface project-registry)
    (project :id payments
      :kind rust-workspace-service
      :root "/Users/jinchen/Projects/xiaojinpro-backend/services/payments"
      :intent ".missiond/intent.lisp"
      :backend ".missiond/backend/payments-backend-blueprint.lisp"
      :status ssot-seeded
      :surface project-registry)
    (project :id asr
      :kind rust-service
      :root "/Users/jinchen/Projects/xiaojinpro-backend/services/asr"
      :intent ".missiond/intent.lisp"
      :backend ".missiond/backend/asr-backend-blueprint.lisp"
      :status ssot-seeded
      :surface project-registry)
    (project :id timeline
      :kind rust-service
      :root "/Users/jinchen/Projects/xiaojinpro-backend/services/timeline"
      :intent ".missiond/intent.lisp"
      :backend ".missiond/backend/timeline-backend-blueprint.lisp"
      :status ssot-seeded
      :surface project-registry)
    (project :id pcea
      :kind rust-vite-app
      :root "/Users/jinchen/Downloads/PCEA develop"
      :intent ".missiond/intent.lisp"
      :backend ".missiond/backend/pcea-backend-blueprint.lisp"
      :frontend ".missiond/frontend/pcea-frontend-blueprint.lisp"
      :status ssot-seeded
      :surface project-registry)
	    (project :id xjp-deploy-center
	      :kind ops-service-source
	      :root "/Users/jinchen/Downloads/xiaojinpro-gateway/xiaojinpro-backend/services/deploy-center"
	      :intent ".missiond/intent.lisp"
	      :status runtime-registered
	      :capability deploy-ops
	      :surface project-registry))

  (service-runtime-universe
    :schema "missiond.service-runtime-universe.v1"
    :rule "Production service runtime facts are Lisp-owned Universe data: project/service roots, domains, deployments, health, DNS capability, and ops owner are visible to resident master and workers through mission_project(action=universe). Secrets stay outside Lisp."
    (service :id auth
      :project xiaojinpro-backend
      :root "/Users/jinchen/Projects/xiaojinpro-backend/services/auth"
      :intent ".missiond/intent.lisp"
      :backend ".missiond/backend/auth-backend-blueprint.lisp"
      :environment production
      :public-base-url "https://auth.xiaojinpro.com"
      :issuer "https://auth.xiaojinpro.com"
      :domains ["auth.xiaojinpro.com"]
      :dns-provider cloudflare
      :dns-capability (:read-inventory true :mutate requires-board-approval :secret-source env)
      :deployment (:substrate kubernetes :namespace production :deployment "xjp-auth-center" :service "xjp-auth-center" :replicas 3 :hpa-min 3 :hpa-max 10 :image "xjp-auth-center:latest" :service-account "xjp-auth-center")
      :proxy (:kind caddy :domain "auth.xiaojinpro.com" :file "/Users/jinchen/Projects/xiaojinpro-backend/services/auth/caddy/Caddyfile" :sse-no-buffer "/auth/login-stream")
      :ports (:http 8081 :metrics 9090 :service 80)
      :health ["/health/live" "/health/ready" "/.well-known/openid-configuration" "/.well-known/jwks.json"]
      :event-ingest (:endpoint "/webhooks/auth-event" :domain system :event ExternalServiceEvent :source auth-audit-events :authority provider-durable-log-first :rule "Auth emits sanitized service events into MissionD EventBus; PTY is diagnostic only and MissionD must not require production probing to observe auth incidents.")
      :dependencies [postgres redis secret-store wechat-open-platform google-oauth sms-provider email-provider]
      :ops-capability deploy-ops
      :source-evidence ["/Users/jinchen/Projects/xiaojinpro-backend/services/auth/k8s/production/configmap.yaml" "/Users/jinchen/Projects/xiaojinpro-backend/services/auth/k8s/production/deployment.yaml" "/Users/jinchen/Projects/xiaojinpro-backend/services/auth/caddy/Caddyfile"]
      :risks [wechat-callback-prod-drift mysql-artifact-cleanup])
    (capability :id cloudflare-dns
      :provider cloudflare
      :default-mode read-only-inventory
      :mutating-policy "Cloudflare DNS mutation requires env/secret binding, deploy-ops capability, and explicit Board approval; workers must report unavailable rather than pretend they can operate DNS when credentials are absent."
      :secrets [CLOUDFLARE_API_TOKEN CLOUDFLARE_ACCOUNT_ID CLOUDFLARE_ZONE_ID]
      :surface service-runtime-universe))

  (capability-governance-policy
    :desc "Lisp-owned capability audit policy; runtime review paths and protected lists are projections, not Rust-only constants."
    :review-sidecar ".missiond/v3/runtime/capability-usage-review.json"
    :protected-tool-patterns ["mission_execution"
                              "mission_intent"
                              "mission_forge_"
                              "mission_sys_"
                              "mission_daemon_update"
                              "mission_health"
                              "mission_power_control"
                              "mission_kb_ops"
                              "mission_audit"
                              "mission_pty_signal"
                              "mission_pty_confirm"
                              "mission_incident"]
    :protected-flow-patterns ["engineering"
                              "F-execution-log-governance"
                              "F-incident-reaction"
                              "F-capability-usage-monitoring"]
    :invariants
      ["mission_capability_usage snapshot/report/candidates/mark/ack MUST project review sidecar location and protected source/target policy from capability-governance-policy."
       "Protected pattern semantics stay explicit: tool patterns ending '_' are prefixes; other tool patterns are exact; flow patterns match exact or prefix."
       "A real MissionD project with .missiond but no capability-governance-policy MUST return V3_BLUEPRINT_CONFIG_ERROR rather than silently using embedded defaults."])

  (memory-kb-policy
    :desc "Lisp-owned memory extraction budget for the memory-kb surface."
    :pending-message-limit 60
    :tool-result-preview-chars 1000
    :assistant-preview-chars 500
    :invariants
      ["mission_memory_pending MUST project batch size and preview truncation lengths from memory-kb-policy."
       "A real MissionD project with .missiond but no memory-kb-policy MUST return V3_BLUEPRINT_CONFIG_ERROR rather than silently using embedded defaults."])

  (learning-engine-policy
    :desc "Lisp-owned autonomous learning engine cadence, pty budget, and low-utility reflection policy."
    :realtime-extraction-timeout-secs 300
    :decision-tier3-timeout-secs 300
    :habit-scan-timeout-secs 600
    :timeline-analysis-interval-secs 43200
    :timeline-analysis-window-hours 12
    :timeline-error-limit 20
    :timeline-llm-sample-limit 50
    :timeline-slow-event-limit 20
    :timeline-slow-threshold-ms 60000
    :idle-explore-interval-secs 7200
    :habit-scan-interval-secs 14400
    :habit-scan-batch-size 5
    :kb-auto-gc-interval-secs 3600
    :kb-consolidation-interval-secs 86400
    :kb-reflection-interval-secs 604800
    :kb-reflection-utility-threshold 0.3
    :kb-reflection-min-access 3
    :kb-reflection-max-entries 20
    :kb-reflection-max-tokens 2000
    :decision-harvest-interval-secs 86400
    :cooccurrence-refresh-interval-secs 21600
    :invariants
      ["LearningEngineRuntimeConfig MUST load learning-engine-policy from .missiond/v3/missiond-blueprint.lisp and fail with V3_BLUEPRINT_CONFIG_ERROR for real MissionD projects whose V3 blueprint or policy block is missing."
       "Realtime extraction, Tier3 decision escalation, and historical habit scan pty.send budgets MUST project from learning-engine-policy."
       "Realtime extraction MUST claim the extraction lane before running pending-message DB probes; pending realtime SQL MUST use EXISTS/LATERAL LIMIT or bounded materialized-candidate shapes instead of global COUNT(DISTINCT)/ROW_NUMBER scans; deep-analysis active-conversation probes MUST use bounded EXISTS/OFFSET checks instead of full message COUNT scans so repeated ticks or status refreshes cannot exhaust the Postgres pool."
       "Learning maintenance cadences (timeline analysis, idle exploration, habit scan, KB auto-GC, KB consolidation, KB reflection, decision harvest, co-occurrence refresh) MUST project from learning-engine-policy."
       "Timeline analysis read windows, event limits, and slow-request threshold MUST project from learning-engine-policy."
       "KB reflection low-utility threshold, minimum access count, max entries, and max_tokens MUST project from learning-engine-policy."
       "Timeline projection SQL MUST cast string-bound since/until parameters as ::timestamptz when comparing against event_log.ts so PG never raises 'operator does not exist: timestamp with time zone >= text' from Timeline Analyst, mission_timeline, or stratified queries."])

  (conversation-ingestion-policy
    :desc "Lisp-owned read-model window and limit defaults for conversation, event, and timeline query surfaces."
    :conversation-get-tail-default 50
    :conversation-search-default-limit 10
    :message-search-default-limit 20
    :context-before-default 3
    :context-after-default 5
    :conversation-events-default-limit 100
    :agent-trajectory-default-limit 200
    :timeline-query-default-limit 50
    :timeline-query-max-limit 200
    :timeline-search-default-limit 20
    :timeline-search-max-limit 100
    :intent-router-model "claude-opus-4.6"
    :intent-router-timeout-ms 10000
    :vision-codex-binary "codex"
    :vision-codex-model "gpt-5.4"
    :vision-codex-idle-timeout-secs 120
    :vision-codex-absolute-timeout-secs 300
    :invariants
      ["mission_conversation_get/search/message_search/context_around MUST project default limits from conversation-ingestion-policy."
       "mission_conversation_events and mission_agent_trajectory MUST project default limits from conversation-ingestion-policy."
       "mission_timeline query/search MUST project default and max limits from conversation-ingestion-policy."
       "UserPromptSubmit context prefetch intent router model and timeout MUST project from conversation-ingestion-policy instead of local claude-opus/10000ms literals."
       "Codex vision worker binary/model/idle timeout and CodexCli absolute timeout MUST project from conversation-ingestion-policy instead of local gpt-5.4/120s/300s literals."
       "A real MissionD project with .missiond but no conversation-ingestion-policy MUST return V3_BLUEPRINT_CONFIG_ERROR rather than silently using embedded defaults."])

  (cli-conversation-ingestion
    :desc "Canonical CLI conversation-log ingestion contract for ClaudeCode, Gemini CLI, and Codex CLI."
    :legacy-aliases ["claude_cli" "pty_jsonl"]
    (source claude-code
      :canonical "claude_code"
      :paths ["~/.claude/projects/**/sessions/*.jsonl"]
      :watcher "crates/missiond-core/src/cc_tasks/watcher.rs"
      :route "crates/missiond-daemon/src/infra/ingestion_router.rs")
    (source gemini-cli
      :canonical "gemini_cli"
      :paths ["~/.gemini/tmp/*/chats/*.json" "~/.gemini/tmp/*/chats/*.jsonl"]
      :watcher "crates/missiond-core/src/gemini_cli/watcher.rs"
      :route "crates/missiond-daemon/src/workers/local/gemini_reconcile_worker.rs")
    (source codex-cli
      :canonical "codex_cli"
      :paths ["~/.codex/state_5.sqlite" "~/.codex/session_index.jsonl" "~/.codex/history.jsonl"]
      :worker "crates/missiond-daemon/src/workers/local/codex_ingestion_worker.rs")
    :invariants
      ["Conversation sources MUST be canonicalized before DB write: claude_code, gemini_cli, or codex_cli."
       "Legacy claude_cli and PTY transport pty_jsonl remain read aliases only; new non-transport source fields MUST name the canonical CLI."
       "mission_pty_status and mission_slots observability MUST be joinable with the latest conversation row by slot/session id and source."
       "mission_slots MUST reject or flag slot_sessions whose conversation source disagrees with the slot engine; stale provider drift must never masquerade as current state."
       "Codex CLI slot_sessions may contain a PTY placeholder id; mission_slots MUST fall back to the latest real codex_cli conversation for the slot project instead of surfacing a messageCount=0 placeholder as the latest durable conversation."
       "Codex CLI message ingestion MUST generate deterministic non-null message_uuid values from thread id, JSONL line number, role, and source event hash so reconcile/backfill cannot repeatedly insert duplicate NULL-uuid rows."
       "Codex CLI background ingestion MUST persist rollout size/mtime/line watermarks and parse only a bounded overlap after the last durable cursor so daemon restarts do not re-hash historical JSONL."
       "When deterministic UUID ingestion meets an older NULL-uuid row with the same session, role, timestamp, and content, the DB layer MUST adopt that existing row by setting message_uuid instead of inserting a new duplicate row."
       "mission_conversation_get MUST defensively coalesce duplicate rows by message_uuid or role/timestamp/content fallback so frontend logs stay readable until historical cleanup is reviewed."
       "mission_conversation_get MUST retrieve tail messages with the indexed (session_id,id) path and assign display seq after duplicate coalescing; it MUST NOT use a ROW_NUMBER window over an entire large Codex/Gemini session."
       "Historical duplicate cleanup is dry-run/report-first; destructive DB cleanup must keep the earliest row in each duplicate group and require an explicit reviewed apply path."
       "Gemini background reconcile MUST use size/mtime companion watermarks to skip already-reconciled old chat files without reparsing full historical transcripts; manual reconcile may force a full scan."
       "Cursor/watermark advancement MUST happen after durable DB write acknowledgement, never before."
       "ClaudeCode provider role normalization MUST be shared by realtime watcher, per-session reconcile, and daily reconcile paths: top-level raw_role=user inside automated slot sessions normalizes to worker_user, interactive Jarvis/user conversations remain user, sidechain progress remains agent_user/agent_assistant, and raw_role is preserved for audit."
       "Historical ClaudeCode role repair is dry-run/report-first through scripts/report-claude-role-attribution.mjs; first pass reports suspected system/user/agent_user drift and never mutates DB."]
    :checker "node scripts/check-v3-cli-conversation-ingestion-isomorphism.mjs")

  (upstream-pty-signatures
    :desc "Provider-aware PTY recognition signatures derived from upstream TUI source instead of screenshot-only heuristics."
    (provider codex-cli
      :canonical "codex_cli"
      :upstream "https://github.com/openai/codex"
      :ref "ff27d01676a93be7467b3893e82f41a7af7e1418"
      :source-paths ["codex-rs/tui/src/status_indicator_widget.rs"
                     "codex-rs/tui/src/chatwidget.rs"
                     "codex-rs/tui/src/bottom_pane/approval_overlay.rs"
                     "codex-rs/tui/src/bottom_pane/chat_composer.rs"]
      :signals [working-status esc-to-interrupt status-details approval-overlay composer-idle])
    (provider gemini-cli
      :canonical "gemini_cli"
      :upstream "https://github.com/google-gemini/gemini-cli"
      :ref "d9f273e44095b742e9ab74241e240c587ae27e64"
      :source-paths ["packages/cli/src/ui/types.ts"
                     "packages/cli/src/ui/components/LoadingIndicator.tsx"
                     "packages/cli/src/ui/components/InputPrompt.tsx"
                     "packages/cli/src/ui/components/messages/DenseToolMessage.tsx"]
      :signals [StreamingState.Idle StreamingState.Responding StreamingState.WaitingForConfirmation Thinking esc-to-cancel CoreToolCallStatus])
    (provider claude-code
      :canonical "claude_code"
      :upstream "/Users/jinchen/Downloads/claudecode/claudecode"
      :source-paths ["src/constants/spinnerVerbs.ts"
                     "src/constants/turnCompletionVerbs.ts"
                     "src/remote/sdkMessageAdapter.ts"
                     "src/cli/print.ts"]
      :signals [spinner-verbs turn-completion-verbs tool-progress auto-mode prompt-footer])
    :output PtyRecognitionSnapshot
    :states [running idle blocked complete unknown]
    :invariants
      ["Codex CLI and Gemini CLI MUST use provider-specific StateParser implementations and MUST NOT fall back to the ClaudeCode parser."
       "mission_pty_status MUST include PtyRecognitionSnapshot with provider, state, confidence, reason, phase/tool/blocked details when available."
       "Autopilot watchdogs MUST treat low-confidence unknown as diagnostic state rather than automatic BoardTask closure evidence."
       "If an upstream TUI signal changes, checker failure is preferred over silent downgrade to generic prompt heuristics."
       "recognize_screen MUST fuse SessionState with screen heuristics: an active processing SessionState (Thinking, Responding, ToolRunning) MUST NOT be demoted to Blocked from screen_fallback confirmation or model-picker text; the fused snapshot is sourced from screen_fused active evidence or session_state, and explicit Confirming SessionState always preserves Blocked."
       "Exited/terminal SessionState overrides stale running screen evidence; mission_pty_status and mission_slots MUST NOT expose recognition.state=running when the durable PTY session state is exited or error."
       "Codex MCP approval menus (`Allow the ... MCP server to run tool`, `Allow for this session`, `enter to submit | esc to cancel`) are explicit blocked TUI source signatures and MUST NOT be demoted to Running just because the SessionState is Thinking."
       "mission_pty_confirm MUST confirm option menus by human-like keyboard navigation (Down/Up then Enter), never by sending numeric shortcut keys; this applies to ClaudeCode, Codex CLI, and Gemini CLI."
       "recognize_claude_code Blocked MUST require explicit confirmation/model-picker UI (Enter to confirm, Do you want to proceed/make this edit/allow/use this api key, Select model, approval request); the bare words `approval` or `permission(s)` -- including the `bypass permissions on` composer-mode footer toggle and historical task-brief prose -- MUST NOT trigger Blocked on Idle or completed screens."]
    :checker "node scripts/check-v3-pty-recognition-isomorphism.mjs")

  (ops-infra
    :desc "Lisp-owned operational scripts for deploy, smoke, and scoped formatting."
    :scripts [scripts/deploy-daemon.sh scripts/cargo-fmt-touched.sh]
    :invariants
      ["Daemon redeploy MUST stay one command: build -> candidate release -> manifest -> active symlink -> launchctl kickstart -> socket wait -> IPC smoke."
       "Active daemon and MCP entrypoints MUST resolve through ~/.xjp-mission/active."
       "Blue-green rollback MUST switch active back to the previous release."
       "Release cleanup MUST keep active, previous, and newest retained releases."
       "IPC smoke MUST retry after socket readiness and then rollback on failure; socket-bound is not enough evidence that the MCP initialize path is ready."
       "Deploy smoke timeout MUST be configurable through MISSIOND_DEPLOY_SMOKE_TIMEOUT so local launchd cold-start races do not force code edits."
       "Deploy scripts MUST emit timing for cargo-build, release-copy, codesign, pre-switch smoke, kickstart, socket wait, post-switch smoke, and cleanup so iteration bottlenecks are observable."
       "Dev-only fast deploy may select debug profile and sccache through explicit operator flags/env, but must preserve release manifest, active symlink, smoke, and rollback semantics unless smoke is explicitly disabled."
       "AST repository-wide startup full sync MUST be opt-in through MISSIOND_AST_FULL_SYNC_ON_STARTUP; routine blue-green restarts stay event-driven and must not rewrite topology KB when no stale code files were synced."
       "Deploy scripts MUST NOT write git state or delete the launchd-owned socket; rollback may restore only the installed binary and restart the launchd job."
       "Rust formatting MUST be scoped to Rust files touched in the current diff, including staged, unstaged, and branch-diff modes."
       "missiond-rustfmt-exempt legacy-large-file facades are skipped only during physical V3 split."
       "rustfmt MUST run with skip_children=true so formatting a crate root cannot recursively churn untouched Rust modules."
       "The no-Rust-files path MUST exit 0 under set -euo pipefail; filters must not turn an empty grep match into a script failure."]
    :checks ["bash -n scripts/deploy-daemon.sh"
             "bash -n scripts/cargo-fmt-touched.sh"
             "scripts/cargo-fmt-touched.sh --check"
             "node scripts/check-v3-ops-infra-isomorphism.mjs"])

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
      :note "V2 architecture maintenance becomes a conservative workflow.lisp muscle memory: nightly evolution now defaults to MissionD V3 SSOT only. It reviews the V3 blueprint, V3 checker output, final convergence static snapshot, and recent commits touching .missiond/v3/**, then writes reports and visible follow-up tasks under risk gates. KB, historical conversations, provider logs, worker telemetry, and Board open-task evidence belong to later explicit memory-audit workflows.")
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
    (v2-item knowledge-memory-and-kb
      :status runtime-projected
      :v2-source ".missiond/v2/intent.lisp :: memory/kb-manager"
      :v3-pillar memory
      :v3-function knowledge-memory
      :surface memory-kb
      :note "KB, beacon, memory, insight, and intent snapshot tools are physically split under the V3 memory-kb surface; memory-kb-policy now projects realtime memory pending batch size and preview truncation budgets into mission_memory runtime.")
    (v2-item project-registry
      :status runtime-projected
      :v2-source ".missiond/v2/intent-worker.lisp :: project-root-spawn-cwd / ProjectRegistry"
      :v3-pillar memory
      :v3-function project-registry
      :surface project-registry
      :note "Project root resolution and registry behavior are physically split and pinned under the V3 project-registry surface; project-registry-policy now projects intent-path candidates and default universe manifest into mission_project init/import_universe/survey runtime.")
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
        :tools [mission_kb_query mission_kb_remember mission_kb_mutate mission_kb_ops mission_beacon
                mission_code_search mission_memory mission_insight mission_intent])
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
      (tool-group capability-audit-tools
        :status code-aligned
        :v2-source ".missiond/v2/intent-capability-governance.lisp"
        :v3-pillar communication
        :v3-function capability-governance
        :surface capability-governance
        :tools [mission_capability_usage mission_audit mission_codex_ops])
      (tool-group sysinfra-control-tools
        :status code-aligned
        :v2-source ".missiond/v2/intent-system-layer.lisp :: sysinfra"
        :v3-pillar operations
        :v3-function sysinfra-control
        :surface sysinfra-control
        :tools [mission_infra_query mission_infra_ops mission_permission_query mission_permission_mutate
                mission_power_control mission_sys_logs mission_sys_config mission_daemon_update
                mission_global_instruction])))

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
        :egress [workflow.lisp workflow_row compiled_yaml run_result]))

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
        :entry [check-staged-source-hygiene task-scope-guard pre-commit-hook]
        :core ((step s1 :logic "inspect staged or supplied files without mutating git")
               (step s2 :logic "reject raw NUL bytes and git diff whitespace errors")
               (step s3 :logic "enforce task contract write-scope and must-not-touch patterns"))
        :egress [source_hygiene_result scope_guard_diagnostics hook_doctor_status])
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
               (step s2 :logic "inspect committed snapshot with git diff-tree --root --no-commit-id -r --name-only <sha>")
               (step s3 :logic "classify files into code/lisp/checker/evidence/docs/other")
               (step s4 :logic "covered when code changes have same-commit Lisp/checker/evidence coverage; lisp-only commits do not recurse")
               (step s5 :logic "create one visible deduped backfill BoardTask for code-only commits"))
        :egress [commit_convergence_report backfill_boardtask mission_master_status.commitConvergence]))

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
        :entry [mission_board.create mission_board.claim mission_board.update mission_board.note_add]
        :core ((step s1 :logic "persist BoardTask status, assignee, dependency, lease, and note state")
               (step s2 :logic "claim only open unclaimed rows and recover stale running rows")
               (step s3 :logic "publish BoardEvent projections for dashboards and autopilot"))
        :egress [board_task board_event board_note autopilot_task_list]))

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
               (step s2 :logic "make top-level decisions and record them before delegation")
               (step s3 :logic "dispatch investigation/context-pack workers before code shards")
               (step s4 :logic "delegate ordinary implementation to Claude/Gemini/Codex pool workers through BoardTask/Autopilot")
               (step s5 :logic "write checkpoint and durable final note after each decision boundary"))
        :egress [master_checkpoint delegated_boardtasks governance_notes mission_master_status])
      (function nightly-evolution
        :surface nightly-evolution-loop
        :entry [night-scheduler mission_nightly_evolution final-convergence-snapshot]
        :core ((step s1 :logic "collect only MissionD V3 blueprint, V3 checker output, final convergence static snapshot, and recent commits touching .missiond/v3/**")
               (step s2 :logic "detect MissionD V3 SSOT loop smells, structure repetition, surface/checker gaps, runtime projection gaps, missing entry/core/egress steps, and repeated Lisp prose")
               (step s3 :logic "classify findings into observe-only, safe-backfill, needs-investigation, architecture-proposal, or requires-user-decision")
               (step s4 :logic "write nightly-evolution report; create visible follow-up tasks only under risk gate")
               (step s5 :logic "surface status through mission_master_status.nightlyEvolution"))
        :egress [nightly_evolution_report proposal_boardtask mission_master_status.nightlyEvolution])
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
      (function knowledge-memory
        :surface memory-kb
        :entry [mission_kb_query mission_kb_remember mission_kb_mutate mission_kb_ops mission_beacon mission_code_search mission_memory mission_insight mission_intent]
        :core ((step s1 :logic "load memory-kb-policy and learning-engine-policy for realtime extraction batch, preview budgets, learning cadences, and pty send budgets")
               (step s2 :logic "resolve project/global memory scope and normalize KB or intent query")
               (step s3 :logic "read or mutate durable knowledge rows through one Lisp-described memory contract")
               (step s4 :logic "project search, beacon, insight, and memory responses into reviewable evidence"))
        :egress [kb_result memory_projection search_hits insight_summary])
      (function project-registry
        :surface project-registry
        :entry [mission_project ProjectRegistry.resolve]
        :core ((step s1 :logic "resolve registered project root from explicit project_id, cwd, or target_project")
               (step s2 :logic "reject ambiguous or outside-root runtime paths before workstation spawn")
               (step s3 :logic "return project metadata usable by request, plan, context, and workstation surfaces"))
        :egress [project_root project_id requested_cwd_policy])
      (function board-frontend
        :surface board-frontend
        :entry [project-blueprint-registry board-blueprint.lisp packages/board]
        :core ((step s1 :logic "register Board's project-local frontend Lisp SSOT without folding it into the backend V3 monolith")
               (step s2 :logic "require Board frontend pillar-flow, implementation-map, and runtime-projection checkers")
               (step s3 :logic "pin frontend code surfaces to Lisp so later workstation shards receive disjoint write scopes")
               (step s4 :logic "project MissionD runtime slot/PTY state into UI code instead of stale static workstation lists"))
        :egress [frontend-blueprint-checks board-ui-surfaces runtime-projection-contract]))

    (pillar communication
      (function conversation-ingestion
        :surface conversation-ingestion
        :entry [mission_conversation_query mission_conversation_analyze mission_conversation_reconcile mission_timeline mission_retrospective_manage mission_embedding_ops]
        :core ((step s1 :logic "load conversation-ingestion-policy for read-model default and max limits")
               (step s2 :logic "ingest or query conversation/session/timeline records by project scope")
	               (step s3 :logic "when mission_conversation_query list is scoped by taskId and conversationType is omitted, query all provider conversation rows for that BoardTask by direct conversations.task_id plus message-anchored BoardTask id fallback, so Claude/Codex/Gemini durable logs remain first-class evidence even after a reused slot is rebound to a later task")
               (step s4 :logic "compaction timeline reconstruction tolerates legacy NULL started_at/message_count rows by coalescing before tuple decode")
               (step s5 :logic "derive analysis, reconciliation, retrospective, and embedding work items")
               (step s6 :logic "surface durable facts for context assembly and later memory projection"))
        :egress [conversation_rows timeline_events retrospective_result embedding_jobs])
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
               (step s2 :logic "route blocked work to the human/orchestrator without mutating the primary artifact")
               (step s3 :logic "return status and unblock hints to Autopilot or request-flow callers"))
        :egress [question_id incident_record decision_stats unblock_hint])
      (function capability-governance
        :surface capability-governance
        :entry [mission_capability_usage mission_audit mission_codex_ops]
        :core ((step s1 :logic "load capability-governance-policy for review sidecar path and protected source/target lists")
               (step s2 :logic "record capability usage, audit facts, and Codex operation acknowledgements")
               (step s3 :logic "bind evidence to plan/execution ids without becoming the primary execution gate")
               (step s4 :logic "return traceable receipts for later learning and report finalization"))
        :egress [capability_receipt audit_record codex_ops_result]))

    (pillar worker-runtime
      (function compute-primitives
        :surface compute-primitives
        :entry [mission_task_submit mission_task_query mission_task_cancel mission_job_poll mission_flow_run mission_pty_spawn mission_pty_send mission_pty_read mission_pty_signal mission_pty_confirm mission_pty_status mission_pty_screenshot mission_slots mission_slot_history mission_agent mission_inbox mission_sonnet_process mission_minimax_process mission_cc_query mission_cc_swarm mission_worker mission_control mission_pause mission_forge_build mission_forge_lint CodexCliStateParser GeminiCliUpstreamStateParser recognize_screen]
        :core ((step s1 :logic "normalize low-level runtime requests into slot, job, task, PTY, flow, forge, or process operations")
               (step s2 :logic "apply project-root, permission, timeout, and pause/control policies before side effects")
               (step s3 :logic "classify provider PTY text into running/idle/blocked/complete/unknown with confidence and reason")
               (step s4 :logic "return durable runtime handles and status without bypassing BoardTask or plan execution when a higher-level surface exists"))
        :egress [runtime_handle job_status pty_snapshot PtyRecognitionSnapshot flow_result forge_result])
      (function skill-runtime
        :surface skill-runtime
        :entry [mission_skill_query mission_skill_context mission_skill_mutate mission_skill_exec]
        :core ((step s1 :logic "resolve skill metadata and context through the skill registry")
               (step s2 :logic "validate mutation or execution request against project and permission policy")
               (step s3 :logic "return skill execution result or context bundle as a runtime receipt"))
        :egress [skill_context skill_mutation skill_execution_receipt])
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
               (step s2 :logic "for mission_daemon_update, return an async logged deploy job for full builds and reserve synchronous restart for skip_build")
               (step s3 :logic "enforce explicit side-effect policy and keep operational state separate from request artifacts")
               (step s4 :logic "return bounded operational status or mutation receipt"))
        :egress [infra_result permission_receipt daemon_update_status global_instruction_state])
      (function ops-infra
        :surface ops-infra
        :entry [scripts/deploy-daemon.sh scripts/cargo-fmt-touched.sh]
        :core ((step s1 :logic "build, backup, codesign, install, kickstart, and smoke daemon as one command")
               (step s2 :logic "retry IPC initialize smoke after socket readiness and rollback on real failure")
               (step s3 :logic "format only Rust files touched in current diff with rustfmt skip_children")
               (step s4 :logic "keep restart-time background indexing event-driven unless an operator explicitly opts into repository-wide AST full sync"))
        :egress [deployed-daemon rollback-result scoped-rustfmt-result])))

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
      :note "V3 physical split: request.rs remains the mission_request action facade plus start/advance/status entry adapter, request/request_artifacts.rs owns request-local paths, request.lisp and lifecycle event rendering, projection planning, pipeline-meta extraction, compat opt-in policy helpers, and JSON artifact status projection, request/respond.rs owns review-response adapter orchestration: approve_intent/approve_plan/execute_plan delegation and blocked-response construction; request/respond/events.rs owns request-local review event sequencing/rendering and next_action projection; request/respond/materialization.rs owns hidden BoardTask anchor creation, request-local plan.lisp materialization/amendment, Plan row insertion, and materialization JSON projection; request/respond/routing.rs owns response parsing, directive/plan ref resolution, Lisp keyword ref scanning, and app... [details: .missiond/v3/evidence/blueprint-notes.lisp#note-001]")

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
      :note "unified-entry-runtime is the daemon-local substrate for F-intent-alignment-plan-execution-loop; mission_request is the user-facing review-packet/respond adapter, while unified_entry.rs is now a thin staged-runtime facade. The V3 physical split is explicit: stages.rs owns FLOW_REF plus s1_message_intake, s3_alignment_review_gate, s4_plan_authoring, s5_plan_review_gate, and s6_execution_runner; planner.rs owns plan_pipeline plus the pure directive/plan/execute argument builders; decorator.rs owns ArtifactScope, build_artifact_refs, decorate, and planner-error envelope projection; unified_entry/tests.rs owns the canonical loop, artifact-ref, pipeline-meta, and decorator regression pins so the facade stays small without losing behavior coverage. run_pipeline dispatches to run_directive_compile_stage, run_plan_compile_stage, and run_plan_execute_stage, then decorate stamps... [details: .missiond/v3/evidence/blueprint-notes.lisp#note-002]")

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
      :note "directive.rs remains the mission_directive action facade plus list/get/version_chain store reads. directive/compile_authoring.rs owns intent-alignment authoring: dry_run emits a deterministic directive-draft Lisp artifact with utterance/source/status; sonnet output is accepted only when it is one balanced Lisp s-expression with head directive|directive-draft|intent-alignment. Persisted directive Lisp is enriched with :directive_id + :version before being surfaced as compiled_sexp(_preview) and before optional file-first writes. The compatibility file writer targets ArtifactKind::IntentAlignment at .missiond/alignment/<topic>/intent-alignment.lisp, never rolls back a committed row on file failure, and review_gate_policy only emits/records gates; it never auto-approves intent. directive/approval_review.rs owns approve/archive/review-resolution transitions, deterministic... [details: .missiond/v3/evidence/blueprint-notes.lisp#note-003]")

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
      :note "compiler_mode=dry_run now renders plan-draft as an executable Lisp scaffold with :target, :objective, and :nodes; execute can derive target_source=plan_hint from plan.sexp_text instead of caller escape parameters. [details: .missiond/v3/evidence/blueprint-notes.lisp#note-004]")

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
      :note "evidence_collector.rs remains the compatibility facade for the verification-receipt evidence surface. evidence_collector/taxonomy.rs owns EVIDENCE_SCHEMA_VERSION and source/kind wire constants; evidence_collector/event_ref.rs owns EventRefStatus live | log | unavailable plus EventRefProvenance live | passive_cache | event_log_query | unavailable; evidence_collector/entry.rs owns EvidenceEntry typed builder/projection; evidence_collector/append.rs owns AppendOutcome and the sidecar append writer; evidence_collector/legacy.rs owns wrap_legacy_record_evidence. evidence_collector/resolver.rs owns EventRefResolver, EVENT_REF_CACHE_CAP = 1024, cache-miss/log-query miss constants, and the bounded event-log query recovery path. wrap_legacy_record_evidence lifts caller-supplied JSON evidence into the typed EvidenceEntry envelope without losing prior fields, keeping plan.rs com... [details: .missiond/v3/evidence/blueprint-notes.lisp#note-005]")

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
	      :note "mission_execution-log is the durable companion-log and live-projection surface for mission_execution. agent_execution/log_surface.rs keeps emit_execution_event plus compatibility re-exports after durable writes succeed; split log modules own paths, storage, mutation, dispatch metadata, counters, status read-model, and session-trace projection. [details: .missiond/v3/evidence/blueprint-notes.lisp#note-006]")

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
	      :note "mission_execution-completion-audit owns the completion durability gate. action_complete records completion facts from the facade, while agent_execution/completion_fields.rs owns VALID_COMMIT_STATUSES, VALID_VERIFIER_STATUSES, VALID_TASK_RUN_VERIFIER_STATUSES, normalize_commit_status, normalize_verifier_status, normalize_task_run_verifier_status, collect_string_list, render_string_list, parse_string_list, and the commit-status-without-hash / commit-status-blocked-without-blocker / scoped-commit-violation finding constants. [details: .missiond/v3/evidence/blueprint-notes.lisp#note-007]")

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
      :note "workflow.rs remains the thin mission_workflow action facade. workflow/store_actions.rs owns list/get/match/apply/record_execution and parse_id_arg; workflow/project_root.rs owns the canonical project-root resolver and the no process-cwd fallback invariant; workflow/compile_methodology.rs owns CompileMode, parse_compile_mode, action_compile_methodology, dry-run preview, deterministic YAML compile, methodology V3 artifact projection, review-gate receipt emission, and count_top_form; workflow/run_methodology.rs owns compiled YAML resolution, mission_flow_run dispatch, parse_run_methodology_record_intent, and methodology_execution_record_payload. workflow/distill.rs owns DistillMode, parse_distill_mode, action_distill, action_distill_dry_run, action_distill_sonnet, evidence sidecar path/read/gate, workflow_sexp JSON extraction, balanced-S-expression validation, name-refer... [details: .missiond/v3/evidence/blueprint-notes.lisp#note-008]")

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
      :note "review-gate is the shared event-bus review layer behind alignment-review-gate, plan-review-gate, workflow review, and the V3 two-gate-default axiom; it must never auto-approve without explicit caller approval plus matching proposal hash. [details: .missiond/v3/evidence/blueprint-notes.lisp#note-009]")

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
      :note "Task-scoped lifecycle events are first-class one-event files: the primary task-scoped path is .missiond/tasks/<wave>/events/<seq>.event.lisp (one lifecycle-event form per file, schema=missiond.task-lifecycle-event.v1, validated by check-task-lifecycle-events as standalone task-scoped event files), and task-runner-append-event allocates the next numeric file under a directory lock, validates the candidate bytes, and atomically creates them via fs.openSync(file, 'wx') when --events-dir is supplied. The legacy task-scoped task-lifecycle-events.lisp ledger is now a compatibility projection/input only: existing --ledger callers keep working unchanged, and task-runner-wave-state reads conventional task-scoped event files when present and falls back to the legacy ledger for historical waves, deduping by event id when both inputs exist. [details: .missiond/v3/evidence/blueprint-notes.lisp#note-010]")

    (surface source-hygiene
      :status "code-aligned"
      :implements [source-hygiene scoped-write-gate]
      :code ["scripts/check-staged-source-hygiene.mjs"
             "scripts/task-scope-guard.mjs"
             "scripts/check-missiond-hooks.mjs"
             "scripts/install-missiond-hooks.mjs"
             ".githooks/pre-commit"
             "scripts/verify-task-runner-batch.mjs"
             "scripts/check-v3-source-hygiene-isomorphism.mjs"]
      :note "check-staged-source-hygiene.mjs is the read-only staged/source preflight: default mode reads staged ACMR files, rejects raw NUL bytes from staged blobs, runs git diff --cached --check, and delegates to task-scope-guard.mjs when --task or MISSIOND_TASK_CONTRACT is set; --files mode checks supplied files without reading git blobs. task-scope-guard.mjs owns task contract write-scope/must-not-touch enforcement for staged and commit modes. .githooks/pre-commit is opt-in per task via MISSIOND_TASK_CONTRACT; check-missiond-hooks.mjs is a read-only doctor and install-missiond-hooks.mjs is the only mutating hook installer. verify-task-runner-batch imports checkSuppliedFiles for source-hygiene fixture coverage without mutating git.")

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
      :note "commit-lisp-convergence-loop is the event-driven code->Lisp backfill muscle. CommitConvergenceService subscribes to SystemEvent::ContextualCommitDetected, resolves project from the committing slot or registry, inspects committed snapshots with git diff-tree --root --no-commit-id -r --name-only <sha>, classifies code/lisp/checker/evidence/doc files, writes commit convergence reports, and creates one visible deduped BoardTask commit-lisp-backfill:<project>:<sha> for code-only commits. Lisp/checker/evidence-only commits do not recurse.")

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
             "scripts/check-v3-nightly-evolution-isomorphism.mjs"
             "scripts/check-v3-code-isomorphism-complete.mjs"]
      :note "nightly-evolution-loop turns resident master self-review into a reusable workflow. NightlyEvolutionService runs observe-first from V3 schedule policy, and mission_nightly_evolution can manually run the same workflow. Its default evidence set is deliberately narrow: MissionD V3 blueprint, V3 checker output, final convergence static snapshot, and recent commits touching .missiond/v3/**. It does not read KB, historical conversations, provider logs, worker telemetry, or Board open tasks unless a later explicit memory-audit workflow asks for them. The report writes .missiond/v3/runtime/nightly-evolution/<date>.report.lisp and only creates visible low-risk follow-up BoardTasks when apply=true and risk gates allow it.")

    (surface context-pack
      :status "code-aligned"
      :implements [multi-agent-context-pack]
      :code ["scripts/check-context-pack.mjs"
             "scripts/context-pack-append.mjs"
             "scripts/context-pack-compile-shards.mjs"
             "scripts/context-pack-materialize-wave.mjs"
             "scripts/context-pack-run-wave.mjs"
             "scripts/lib/v3_workstation_runtime.mjs"
             "scripts/check-v3-context-pack-isomorphism.mjs"]
      :note "Context-pack is the V3 high-density planning surface for two-stage parallel work: context investigators append claim/observation/anchor/shard-proposal/conflict entries to .missiond/tasks/<wave>/context-pack.lisp without code edits, then an orchestrator/integrator appends integration-plan with accepted-shards and dispatch-groups. Mapped dispatch groups use (group :id <id> :shards [...]) so scripts/context-pack-compile-shards.mjs can project the Lisp plan into dispatchable_groups for code workers; legacy bare group ids remain names_only for older packs. [details: .missiond/v3/evidence/blueprint-notes.lisp#note-011]")

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
	    :note "mission_compute_slot and mission_task_delegate accept model/model_profile; coder/researcher default to Claude Code Default(Opus 4.7/1M) by omitting --model. mission_task_delegate also accepts two-stage delegation metadata (task_class, pool_hint, engine_hint, context_pack_path, read_scope, write_scope, must_not_touch, acceptance) and records it into the BoardTask description for Autopilot worker prompts; the scope_semantics contract separates readable evidence from writable scope and prevents must_not_touch from being misread as a read ban. main.rs startup SlotManager registration loads WorkstationRuntimeConfig and generates persistent SlotTaskConfig rows by iterating workstation-config startup-slot entries; ClaudeCode startup slots project their model_profile through spawn_model_for_profile, so arch maintenance and Lisp survey no longer hardcode claude-sonnet-4-6 or local timeout literals. [details: .missiond/v3/evidence/blueprint-notes.lisp#note-012]")

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
      :note "workstation-pool is the compact V3 compute-account SSOT. It declares ClaudeCode Opus/Sonnet lanes, Gemini read-only lanes, and the non-shard Codex master lane; runtime projection feeds SlotManager, PTYSpawnOptions, Autopilot routing, mission_compute_slot list, and mission_slots legacy-Sonnet filtering. mission_slots MUST project activeBoardTaskId/currentTaskId and activeBoardTask by joining running BoardTasks on assignee or pty_slot claim so the Board cockpit can show what each visible PTY is actually doing. [details: .missiond/v3/evidence/workstation-pool.lisp]")

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
      :note "resident-master-control promotes Codex to a non-shard orchestrator. Runtime projection starts GPT-5.5 xhigh read-only Codex, writes phaseful checkpoints, exposes mission_master_status and mission_convergence_status, supervises commit-lisp-convergence-loop and nightly-evolution-loop status, and keeps provider logs as completion authority while PTY remains diagnostic. Active BoardTask objectives override periodic self-review scope and must be followed as the load-bearing objective; if the master says it will create/update a BoardTask, it must perform the Board MCP mutation before final response.")

    (surface autopilot-runtime
      :status "code-aligned"
      :implements [delegated-boardtask-runtime event-driven-autopilot-handoff]
      :code ["crates/missiond-daemon/src/bus/v2_subscribers.rs"
             "crates/missiond-daemon/src/engine/intent_engine/autopilot.rs"
             "crates/missiond-daemon/src/handlers/compute/task_delegate.rs"
             "crates/missiond-daemon/src/handlers/knowledge/board/events.rs"
             "crates/missiond-core/src/event/events/board.rs"
             "crates/missiond-core/src/event/events/slot.rs"
             "scripts/check-v3-autopilot-runtime-isomorphism.mjs"]
      :note "autopilot-runtime is the event-driven muscle layer for delegated BoardTasks. task_delegate and mission_board_create publish BoardEvent::TaskCreated; v2_subscribers owns the event-bus nerves v2_autopilot_board_event and v2_autopilot_slot_event, which wake board_dispatch_notify on BoardEvent::TaskCreated, reopened BoardEvent status updates, and SlotEvent::BecameIdle, then ack immediately without running pty.send inline. The dedicated Autopilot task remains the only prompt/close owner: it claims eligible open BoardTasks, derives leases/timeouts from V3 policy, holds a per-slot dispatch guard across state.pty.send, emits SlotEvent::TaskDispatched, synthesizes mission_execution completion when needed, and closes/preserves the BoardTask according to execution-ownership delegated-boardtask. This preserves the event-bus causal chain while keeping long-running worker interaction outside subscriber ack paths.")

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
      :note "workstation-dispatch is the substrate called by mission_plan execute_internal after target=mission_task_delegate is selected; workstation-config owns slot/model/prompt setup, while this surface owns the WorkstationDispatchOutcome response vocabulary and the handoff contract. The rendered brief MUST carry the forbidden git state mutations invariant (git stash / git reset / git checkout / git restore are off-limits unless the task contract explicitly owns the operation) so a delegated worker that meets a dirty worktree stops and adds a BoardTask note instead of silently rewinding shared state. [details: .missiond/v3/evidence/blueprint-notes.lisp#note-013]")

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
             "crates/missiond-mcp/src/tools/knowledge/board.rs"
             "scripts/check-v3-board-isomorphism.mjs"]
      :note "mission_board is the durable BoardTask coordination surface underneath delegated ClaudeCode work: MCP exposes query/create/update/delete/claim/decompose/retry/note_add with a generated schema from .missiond/intent-tools.lisp. [details: .missiond/v3/evidence/blueprint-notes.lisp#note-014]")

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
	      :note "Runtime-projected V3 destination for memory/KB tools. memory-kb-policy and learning-engine-policy own realtime extraction budgets, pty send budgets, cadences, bounded SQL probes, and physical split ownership across kb/* modules. Conversation history distillation is intentionally deferred behind .missiond/workflows/conversation-memory-distillation.lisp: default mode produces candidate-memory / infrastructure issue inventory only, rejects facts already superseded by project SSOT Lisp, and does not write or delete KB until log role attribution and project SSOT coverage are stable. [details: .missiond/v3/evidence/blueprint-notes.lisp#note-015]")

    (surface project-registry
      :status "code-aligned"
      :implements [project-registry project-root-resolution service-runtime-universe]
      :code ["crates/missiond-daemon/src/context/v3_blueprint_runtime.rs"
             "crates/missiond-daemon/src/handlers/knowledge/project.rs"
             "crates/missiond-daemon/src/handlers/knowledge/project/registry.rs"
             "crates/missiond-daemon/src/handlers/knowledge/project/context.rs"
             "crates/missiond-daemon/src/handlers/knowledge/project/universe.rs"
             "crates/missiond-daemon/src/handlers/knowledge/project/survey.rs"
             "crates/missiond-daemon/src/handlers/knowledge/project/vault.rs"
             "crates/missiond-core/src/types/project.rs"
             "crates/missiond-daemon/src/slot_orchestrator/project_root.rs"
             "crates/missiond-mcp/src/tools/knowledge/project.rs"
             "scripts/check-v3-project-registry-isomorphism.mjs"]
      :note "Code-aligned destination for project registry/root resolution. project.rs is the mission_project facade; project/registry.rs owns list/get/set_active/sync/init/import_universe; project/universe.rs owns mission_project(action=universe) and projects service-runtime-universe entries such as auth production domain/deployment/DNS capability to master, workers, and Board System. [details: .missiond/v3/evidence/blueprint-notes.lisp#note-016]")

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
             "scripts/check-v3-conversation-ingestion-isomorphism.mjs"
             "scripts/check-v3-cli-conversation-ingestion-isomorphism.mjs"]
      :note "Runtime-projected V3 destination for conversation/session/timeline/retrospective/embedding public tools. context/v3_blueprint_runtime.rs projects conversation-ingestion-policy read-model default and max limits into conversation/query.rs, conversation/events.rs, and timeline.rs, projects context prefetch intent-router model/timeout into context/context_pipeline.rs, and projects Codex vision worker binary/model/idle/absolute timeout into workers/codex/vision_worker.rs plus llm/codex_cli.rs; conversation.rs is the thin conversation-ingestion facade; conversation/router.rs owns mission_conversation_query, mission_conversation_analyze, and mission_retrospective_manage consolidated routing; conversation/query.rs owns read-model query actions including list/get/search/message_search/user_index/labels/context; conversation/events.rs owns analysis/event egress including conver... [details: .missiond/v3/evidence/blueprint-notes.lisp#note-017]")

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
             "crates/missiond-daemon/src/workers/sonnet/translation_worker.rs"
             "crates/missiond-daemon/src/llm/xjp_router_client.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan/router_policy_dry_run.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan/router_policy_dry_run/predicate.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan/router_policy_dry_run/readiness.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan/router_policy_dry_run/descriptor.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan/router_policy_dry_run/schema_parser.rs"
             "scripts/check-router-policy.mjs"
             "scripts/check-router-backend-registry.mjs"
             "scripts/check-router-dispatch-descriptor.mjs"
             "scripts/check-v3-router-policy-isomorphism.mjs"]
      :note "Runtime-projected V3 destination for the V2 router-policy dry-run chain and public router chat tools. router-runtime-policy owns default chat/flow/Sonnet models, BoardTask ANTHROPIC_MODEL override routing, GeminiClient request queue timeouts, Gemini CLI absolute/tool-exec timeouts, Gemini File API upload/poll timeouts, queued Sonnet quota throttle, xjp-router embedding timeout default, and token/timeout/compression budgets through RouterRuntimeConfig. router_chat.rs is the thin router-policy facade; router_chat/chat.rs owns mission_router_chat request normalization, context injection, LLM dispatch, persistence, and response projection; router_chat/files.rs owns attachment denylist and Gemini File API policy; router_chat/manage.rs owns mission_router_chat_manage history/list/delete/clear/delete_message/restore/stats/compress. embedding_worker.rs owns LlmConfig/GeminiCl... [details: .missiond/v3/evidence/blueprint-notes.lisp#note-018]")

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

    (surface capability-governance
      :status "code-aligned"
      :implements [capability-usage audit codex-ops]
      :code ["crates/missiond-mcp/src/tools/comm/capability_usage.rs"
             "crates/missiond-mcp/src/tools/comm/audit.rs"
             "crates/missiond-mcp/src/tools/comm/codex_ops.rs"
             "crates/missiond-daemon/src/handlers/mod.rs"
             "crates/missiond-daemon/src/context/v3_blueprint_runtime.rs"
             "crates/missiond-daemon/src/handlers/comm/capability_usage.rs"
             "crates/missiond-daemon/src/handlers/comm/capability_usage/runtime.rs"
             "crates/missiond-daemon/src/handlers/comm/audit.rs"
             "crates/missiond-daemon/src/handlers/comm/codex_ops.rs"
             "scripts/check-v3-capability-governance-isomorphism.mjs"]
      :note "Runtime-projected V3 destination for capability usage, audit, and Codex ops surfaces. capability_usage.rs is the thin capability-governance facade; capability_usage/runtime.rs owns snapshot/report/candidates/mark/ack, six source lanes, semantic hint merge review, protected source/target policy, review sidecar persistence, and non-blocking observability emissions; context/v3_blueprint_runtime.rs projects capability-governance-policy review sidecar path plus protected source/target lists into mission_capability_usage runtime; audit.rs owns mission_audit trace/detail/stats/export plus legacy mission_audit_* compatibility; codex_ops.rs owns mission_codex_ops recent/thread/tool_stats over codex_cli conversations.")

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
      :note "Code-aligned V3 destination for low-level worker runtime primitives. task.rs owns mission_task_submit/query/cancel plus async/sync/status/list/ack/track and TaskEvent::Created egress, and projects auto-spawn tracked PTY wait_for_idle timeout from compute-runtime-policy; job.rs owns mission_job_poll poll/list/cancel over AsyncJobStatus; flow_run.rs owns mission_flow_run BoardTask-backed flow execution and project-root resolution; engine/flow/mod.rs owns FlowDefinition shape constants, engine/flow/loader.rs loads flow-runtime-policy through context/v3_blueprint_runtime.rs and projects missing YAML node defaults while preserving explicit fields; pty.rs owns mission_pty_spawn/send/read/signal/confirm/status/screenshot plus kill/interrupt/read screen-history-logs, task requeue, and permission learning; process.rs owns mission_agent spawn/kill/restart/list and projects trac... [details: .missiond/v3/evidence/blueprint-notes.lisp#note-019]")

    (surface skill-runtime
      :status "code-aligned"
      :implements [skill-query skill-context skill-mutate skill-exec]
      :code ["crates/missiond-daemon/src/handlers/knowledge/skill.rs"
             "crates/missiond-daemon/src/handlers/knowledge/skill/query.rs"
             "crates/missiond-daemon/src/handlers/knowledge/skill/context.rs"
             "crates/missiond-daemon/src/handlers/knowledge/skill/mutate.rs"
             "crates/missiond-daemon/src/handlers/knowledge/skill/exec.rs"
             "crates/missiond-mcp/src/tools/knowledge/skill.rs"
             "scripts/check-v3-skill-runtime-isomorphism.mjs"]
      :note "Code-aligned V3 destination for skill registry and skill execution behavior. skill.rs is the thin mission_skill facade for consolidated mission_skill_query, mission_skill_context, mission_skill_mutate, and direct legacy skill tool names; skill/query.rs owns list/search/topics/actions/stats, FTS/vector ranking, topic hit recording, workflow action projection, and execution stats egress; skill/context.rs owns context build/resolve, skill dependency expansion, infra and KB dependency aggregation, and optional BoardTask context projection; skill/mutate.rs owns upsert/record/render/rollback, topic auto-create, block writes, materialization, skill version rollback, and embedding refresh through ProcessSkillTopic; skill/exec.rs owns mission_skill_exec and execute_workflow result/error egress. Additional skill policy knobs must land as dedicated V3 policy sections with checker pins before runtime code reads them.")

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
             "scripts/check-v3-sysinfra-control-isomorphism.mjs"]
      :note "Code-aligned V3 destination for sysinfra MCP behavior not covered by ops-infra scripts. infra.rs owns mission_infra_query/ops list/get/health/reachability/diagnose and reachability probes; permission.rs owns mission_permission_query/mutate including get, learned_list, merged_for_slot, set_role, set_slot, auto_allow, reload, and revoke; power.rs owns mission_power_control status/wake/suspend and removes power from the legacy misc hot path; system.rs owns mission_sys_logs, mission_sys_config, mission_daemon_update, and missiond-blue-green-self-update. mission_daemon_update full build MUST start scripts/deploy-daemon.sh as a detached async logged job to stay below MCP tools/call timeout and survive daemon kickstart; deploy-daemon.sh MUST co-build missiond and mission-mcp into one blue-green release so newly declared MCP tools are not left behind the daemon release. skip_build remains the synchronous already-built artifact restart path. global_instruction.rs owns mission_global_instruction read/edit/manual-reload.")

    (surface ops-infra
      :status "code-aligned"
      :implements [ops-infra]
      :code ["scripts/deploy-daemon.sh"
             "scripts/cargo-fmt-touched.sh"
             "crates/missiond-daemon/src/main.rs"
             "crates/missiond-daemon/src/workers/local/ast_sync_worker.rs"
             "scripts/check-v3-ops-infra-isomorphism.mjs"
             "scripts/check-missiond-blue-green-deploy.mjs"]
      :note "ops-infra owns deploy-daemon.sh plus scoped Rust formatting and restart-time background CPU policy. deploy-daemon.sh builds paired missiond/mission-mcp release candidates under ~/.xjp-mission/releases/<release-id>, writes release-manifest.json, switches ~/.xjp-mission/active, keeps stable entrypoints through active, kickstarts launchd, runs MCP smoke, rolls back to previous active on failure, and cleans retained releases. cargo-fmt-touched.sh formats only touched Rust files with skip_children=true and skips only explicit missiond-rustfmt-exempt facades. main.rs keeps repository-wide AST startup full sync opt-in via MISSIOND_AST_FULL_SYNC_ON_STARTUP, and ast_sync_worker skips topology KB rewrites when no stale files were synced.")

    (surface missiond-blue-green-self-update
      :status "code-aligned"
      :implements [blue-green-self-update release-manifest release-cleanup rollback]
      :code ["scripts/deploy-daemon.sh"
             "scripts/check-missiond-blue-green-deploy.mjs"
             "scripts/check-v3-ops-infra-isomorphism.mjs"
             "scripts/check-v3-sysinfra-control-isomorphism.mjs"]
      :note "MissionD self-update is owned as a blue-green release workflow. Release candidates are immutable directories under ~/.xjp-mission/releases/<release-id>; the active symlink is the only switch; daemon and MCP entrypoints both resolve through active so they share one release-manifest.json. The deploy path supports legacy direct-binary migration, pre-switch MCP smoke, post-switch daemon IPC smoke, previous-release rollback, cleanup-only dry-run/apply, and retention of active/previous/newest releases.")
    )

  (compression-contract
    :v1 "Organized by .missiond/v1/manifest.lisp; root files remain compatibility paths."
    :v2 "Kept as historical source index, implementation status, and wave evidence."
    :v3 "Small executable contracts only: request, artifact, state-machine, policy, pillar-flow-map, implementation map."
    :checks ["node scripts/check-lisp-blueprint-compression.mjs"
             "node scripts/check-architecture-lisp.mjs --no-structure .missiond/v3/missiond-blueprint.lisp"
             "node scripts/check-v3-pillar-flow-schema.mjs"
             "node scripts/check-v3-v2-coverage.mjs"
             "node scripts/check-v3-runtime-path-hygiene.mjs"
             "node scripts/check-v3-conversation-ingestion-isomorphism.mjs"
             "node scripts/check-v3-cli-conversation-ingestion-isomorphism.mjs"
             "node scripts/check-v3-pty-recognition-isomorphism.mjs"
             "node scripts/check-v3-capability-governance-isomorphism.mjs"
             "node scripts/check-v3-compute-primitives-isomorphism.mjs"
             "node scripts/check-v3-sysinfra-control-isomorphism.mjs"
             "node scripts/check-v3-router-policy-isomorphism.mjs"
             "node scripts/check-v3-request-lisp-isomorphism.mjs"
             "node scripts/check-v3-unified-entry-isomorphism.mjs"
             "node scripts/check-v3-file-artifacts-isomorphism.mjs"
             "node scripts/check-v3-intent-alignment-isomorphism.mjs"
             "node scripts/check-v3-plan-execution-isomorphism.mjs"
             "node scripts/check-v3-evidence-collector-isomorphism.mjs"
             "node scripts/check-v3-mission-execution-isomorphism.mjs"
             "node scripts/check-v3-workflow-isomorphism.mjs"
             "node scripts/check-v3-review-gate-isomorphism.mjs"
             "node scripts/check-v3-task-lifecycle-isomorphism.mjs"
             "node scripts/check-v3-memory-kb-isomorphism.mjs"
             "node scripts/check-v3-project-registry-isomorphism.mjs"
             "node scripts/check-v3-skill-runtime-isomorphism.mjs"
             "node scripts/check-v3-cascade-governance-isomorphism.mjs"
             "node scripts/check-v3-incident-governance-isomorphism.mjs"
             "node scripts/check-v3-source-hygiene-isomorphism.mjs"
	             "node scripts/check-v3-context-pack-isomorphism.mjs"
	             "node scripts/check-v3-workstation-config-isomorphism.mjs"
	             "node scripts/check-v3-workstation-pool-isomorphism.mjs"
	             "node scripts/check-v3-master-control-isomorphism.mjs"
	             "node scripts/check-v3-direct-code-drift-policy.mjs"
	             "node scripts/check-v3-commit-convergence-loop.mjs"
	             "node scripts/check-v3-nightly-evolution-isomorphism.mjs"
		             "node scripts/check-v3-autopilot-runtime-isomorphism.mjs"
             "node scripts/check-v3-workstation-dispatch-isomorphism.mjs"
             "node scripts/check-v3-board-isomorphism.mjs"
             "node scripts/check-frontend-board-lisp-schema.mjs"
             "node scripts/check-frontend-board-code-isomorphism.mjs"
             "node scripts/check-frontend-board-runtime-projection.mjs"
             "node scripts/check-v3-ops-infra-isomorphism.mjs"
             "node scripts/check-v3-request-flow-smoke.mjs"
             "node scripts/check-v3-code-isomorphism-complete.mjs"
             "node scripts/check-v3-final-convergence.mjs"]
    :rule "New runtime work should cite v3 first, then v2 source-index for historical evidence."))
