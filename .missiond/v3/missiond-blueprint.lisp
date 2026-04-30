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
      :completion-rule "complete iff final-report is finalized, final commit matches lineage, and required receipts are valid"))

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
    :flow [parallel-claims parallel-observations shard-proposals conflict-notes integration-plan compile-shards materialize-wave dispatch-code-workers verify-and-finalize]
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
       "context-pack-materialize-wave MUST refuse names-only dispatch groups and may only project mapped integration-plan shards into task-runner manifest + task-contract files; it does not dispatch workers."
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
    (model-profile daily-sonnet
      :applies-to [ops low-risk-maintenance]
      :spawn-model-arg "sonnet"
      :rule "Use only when the task or caller explicitly asks for Sonnet-class daily work.")
    (slot-template coder
      :role coder
      :default-model-profile coding-default-opus-4-7
      :mcp-config "/Users/jinchen/.xjp-mission/xjp-mcp-config.json")
    (slot-template researcher
      :role coder
      :default-model-profile coding-default-opus-4-7
      :mcp-config "/Users/jinchen/.xjp-mission/xjp-mcp-config.json")
    (slot-template ops
      :role operator
      :default-model-profile daily-sonnet
      :mcp-config "/Users/jinchen/.xjp-mission/xjp-mcp-config.json")
    (timeout-policy boardtask-dispatch
      :default_secs 1800
      :min_secs 60
      :max_secs 7200
      :watchdog_grace_secs 120
      :missing_session_probe_secs 120)
    :invariants
      ["code and research dynamic slots MUST NOT hardcode --model sonnet"
       "model=\"default\" and model_profile=coding-default-opus-4-7 both mean no CLI --model override"
       "caller-supplied model wins over model_profile, but must be a single shell token"
       "task_delegate must pass model/model_profile through to compute_slot and must not reuse an idle slot with a conflicting model override"
       "Project-bound workstation spawn MUST sync MissionD Claude hooks into <project>/.claude/settings.local.json before PTY start and MUST inject MISSION_IPC_ENDPOINT into the slot env; this preserves global ~/.claude/settings.json while making SessionStart UUID capture and UserPromptSubmit context prefetch local, idempotent, and project-scoped"
       "Autopilot pty.send budget MUST project from BoardTask.timeout_secs (default 1800s, clamped 60..7200) — never a fixed 600_000ms — so a delegated long-running task gets the timeout the delegator already declared"
       "Smart watchdog idle-recovery threshold MUST equal the projected pty.send budget plus a small grace (default 120s); only the no-PTY-session branch may reclaim sooner so a missing process can never wedge the slot"
       "Autopilot BoardTask claim lease MUST equal the smart-watchdog idle-recovery threshold (projected pty.send budget plus grace); the legacy fixed 20-minute lease is forbidden because it lets the watchdog reclaim a slot whose claim is still legitimately ticking inside the declared timeout"]
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
         :blocked "If the task transitioned to Blocked (e.g. mission_question_create) during execution, Autopilot preserves the Blocked state on pty.send return and never overwrites it with done.")
      :dispatch-guard
        "The per-slot dispatch guard MUST be held across the entire state.pty.send call; the legacy release-before-send pattern allowed a second caller to dispatch to the same slot mid-flight. The guard is per-slot, so holding it does not starve callers targeting other slots."
      :concurrent-slot-dispatch
        "Autopilot dispatch_board_tasks MUST start state.pty.send work concurrently across different slots within a single dispatch tick; the legacy serial loop awaited one slot's pty.send before any other slot's send could begin, which starved every other ready slot for the duration of one slot's send. The implementation MUST hand each ready BoardTask's send + post-send tail to a tokio::task::JoinSet task with an OwnedSlotDispatchGuard moved in, so different-slot sends start in the same tick while same-slot exclusion still covers the entire send + close-owner / KB-feedback / deploy-review sequence. The outer dispatch_board_tasks MUST drain the JoinSet via join_next so quota / global-pause / KB-feedback / retry semantics still complete before the dispatch tick returns."
      :restart-recovery
        "Restart recovery MUST clear stale slot-dyn-* BoardTask assignee pins when the runtime slot is absent and the dynamic_slots row is not active, using BoardStore::clear_board_task_assignee before normal no-assignee routing resumes."
      :rationale
        "Wave33 evidence: a delegated BoardTask was sent twice — once via spawner.initial_prompt fire-and-forget, then again via Autopilot pty.send — and the slot's TextOutputEvent::Complete arrived without Autopilot transitioning the BoardTask to done. Single ownership of prompt+close eliminates the orphaned-task class entirely."))

  (ops-infra
    :desc "Lisp-owned operational scripts for deploy, smoke, and scoped formatting."
    :scripts [scripts/deploy-daemon.sh scripts/cargo-fmt-touched.sh]
    :invariants
      ["Daemon redeploy MUST stay one command: build -> backup -> codesign -> atomic install -> launchctl kickstart -> socket wait -> IPC smoke."
       "IPC smoke MUST retry after socket readiness and then rollback on failure; socket-bound is not enough evidence that the MCP initialize path is ready."
       "Deploy smoke timeout MUST be configurable through MISSIOND_DEPLOY_SMOKE_TIMEOUT so local launchd cold-start races do not force code edits."
       "Deploy scripts MUST NOT write git state or delete the launchd-owned socket; rollback may restore only the installed binary and restart the launchd job."
       "Rust formatting MUST be scoped to Rust files touched in the current diff, including staged, unstaged, and branch-diff modes."
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
    (v2-item context-pack-two-stage-parallel-work
      :status code-aligned
      :v2-source ".missiond/v2/intent.lisp :: task-runner loop evidence + wave29 context-pack upgrade"
      :v3-pillar coordination
      :v3-function context-pack
      :surface context-pack
      :note "Shared-memory append practice is lifted into the V3 two-stage context-pack surface.")
    (v2-item mission-board-coordination
      :status code-aligned
      :v2-source ".missiond/v2/intent-flow.lisp :: board-task-main-lifecycle"
      :v3-pillar coordination
      :v3-function mission-board
      :surface mission_board
      :note "V2 board task lifecycle and claim mechanics converge into mission_board.")
    (v2-item claudecode-workstation-config
      :status code-aligned
      :v2-source ".missiond/v2/intent-worker.lisp :: claudecode-workstation-orchestration"
      :v3-pillar workstation
      :v3-function workstation-config
      :surface workstation-config
      :note "V2 workstation policy now has explicit V3 model/profile, timeout, prompt, ownership, and close-owner contracts.")
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
      :status designed
      :v2-source ".missiond/v2/intent.lisp :: memory/kb-manager"
      :v3-pillar memory
      :v3-function knowledge-memory
      :surface memory-kb
      :note "KB, beacon, memory, insight, and intent snapshot tools are visible in V3 but still need physical/runtime Lisp alignment.")
    (v2-item project-registry
      :status designed
      :v2-source ".missiond/v2/intent-worker.lisp :: project-root-spawn-cwd / ProjectRegistry"
      :v3-pillar memory
      :v3-function project-registry
      :surface project-registry
      :note "Project root resolution and registry behavior are mapped for later runtime projection.")
    (v2-item conversation-ingestion
      :status designed
      :v2-source ".missiond/v2/intent-worker.lisp :: conversation-jsonl-ingest / session organizer"
      :v3-pillar communication
      :v3-function conversation-ingestion
      :surface conversation-ingestion
      :note "Conversation, timeline, retrospective, and embedding ingestion remain designed V3 surfaces.")
    (v2-item router-policy-dry-run-chain
      :status designed
      :v2-source ".missiond/v2/intent.lisp :: router-policy-v1 / router-backend-readiness-loop / router-dispatch-descriptor-loop"
      :v3-pillar communication
      :v3-function router-policy
      :surface router-policy
      :note "Router policy remains advisory/dry-run; V3 records the destination before any runtime replacement work.")
    (v2-item question-incident-governance
      :status designed
      :v2-source ".missiond/v2/intent-worker.lisp :: system-support/incidents + question flow"
      :v3-pillar communication
      :v3-function incident-question-governance
      :surface incident-governance
      :note "Question, incident, LLM trace, Gemini auth, and decision stats are grouped for later convergence.")
    (v2-item capability-governance
      :status designed
      :v2-source ".missiond/v2/intent-capability-governance.lisp"
      :v3-pillar communication
      :v3-function capability-governance
      :surface capability-governance
      :note "Capability usage, audit, and Codex ops are mapped but not yet physically same-shaped as V3.")
    (v2-item compute-primitives
      :status designed
      :v2-source ".missiond/v2/intent-worker.lisp :: pty / llm / worker / engine runtime"
      :v3-pillar worker-runtime
      :v3-function compute-primitives
      :surface compute-primitives
      :note "PTY, job, flow, forge, cc, process, and low-level worker tools remain a designed runtime-primitives surface.")
    (v2-item skill-runtime
      :status designed
      :v2-source ".missiond/v2/intent-worker.lisp :: skill workflow executor"
      :v3-pillar worker-runtime
      :v3-function skill-runtime
      :surface skill-runtime
      :note "Skill query/context/mutate/exec has a V3 destination before code split.")
    (v2-item cascade-universe-governance
      :status designed
      :v2-source ".missiond/v2/intent-event-bus.lisp :: cascade/control tree"
      :v3-pillar worker-runtime
      :v3-function cascade-governance
      :surface cascade-governance
      :note "Universe graph and cascade tools are mapped as governance/runtime, not left as loose legacy tools.")
    (v2-item sysinfra-control
      :status designed
      :v2-source ".missiond/v2/intent-system-layer.lisp :: system/sysinfra tools"
      :v3-pillar operations
      :v3-function sysinfra-control
      :surface sysinfra-control
      :note "Permission, power, daemon update, and global instruction tools are mapped for later V3 projection.")

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
        :status code-aligned
        :v2-source ".missiond/v2/intent-worker.lisp :: claudecode-workstation-orchestration"
        :v3-pillar workstation
        :v3-function workstation-config
        :surface workstation-config
        :tools [mission_compute_slot mission_task_delegate])
      (tool-group compute-runtime-tools
        :status designed
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
        :status designed
        :v2-source ".missiond/v2/intent.lisp :: memory/kb-manager"
        :v3-pillar memory
        :v3-function knowledge-memory
        :surface memory-kb
        :tools [mission_kb_query mission_kb_remember mission_kb_mutate mission_kb_ops mission_beacon
                mission_code_search mission_memory mission_insight mission_intent])
      (tool-group project-registry-tools
        :status designed
        :v2-source ".missiond/v2/intent-worker.lisp :: project registry"
        :v3-pillar memory
        :v3-function project-registry
        :surface project-registry
        :tools [mission_project])
      (tool-group skill-runtime-tools
        :status designed
        :v2-source ".missiond/v2/intent-worker.lisp :: skill workflow executor"
        :v3-pillar worker-runtime
        :v3-function skill-runtime
        :surface skill-runtime
        :tools [mission_skill_query mission_skill_context mission_skill_mutate mission_skill_exec])
      (tool-group cascade-runtime-tools
        :status designed
        :v2-source ".missiond/v2/intent-event-bus.lisp :: cascade"
        :v3-pillar worker-runtime
        :v3-function cascade-governance
        :surface cascade-governance
        :tools [mission_universe_graph mission_cascade_plan mission_cascade_trigger mission_cascade_lint])
      (tool-group conversation-ingestion-tools
        :status designed
        :v2-source ".missiond/v2/intent-worker.lisp :: conversation-jsonl-ingest"
        :v3-pillar communication
        :v3-function conversation-ingestion
        :surface conversation-ingestion
        :tools [mission_conversation_query mission_conversation_analyze mission_conversation_reconcile
                mission_timeline mission_retrospective_manage mission_embedding_ops])
      (tool-group router-policy-tools
        :status designed
        :v2-source ".missiond/v2/intent.lisp :: router-policy-v1"
        :v3-pillar communication
        :v3-function router-policy
        :surface router-policy
        :tools [mission_router_chat mission_router_chat_manage])
      (tool-group question-incident-tools
        :status designed
        :v2-source ".missiond/v2/intent-worker.lisp :: incident/question"
        :v3-pillar communication
        :v3-function incident-question-governance
        :surface incident-governance
        :tools [mission_question mission_llm_trace mission_decision_stats mission_gemini_auth mission_incident])
      (tool-group capability-audit-tools
        :status designed
        :v2-source ".missiond/v2/intent-capability-governance.lisp"
        :v3-pillar communication
        :v3-function capability-governance
        :surface capability-governance
        :tools [mission_capability_usage mission_audit mission_codex_ops])
      (tool-group sysinfra-control-tools
        :status designed
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
        :egress [source_hygiene_result scope_guard_diagnostics hook_doctor_status]))

    (pillar coordination
      (function context-pack
        :surface context-pack
        :entry [context-pack-append context-pack-compile-shards context-pack-materialize-wave]
        :core ((step s1 :logic "append claim/observation/anchor/shard-proposal/conflict entries with locked seq allocation")
               (step s2 :logic "validate accepted shard references and non-overlap")
               (step s3 :logic "compile integration-plan dispatch groups for code workers")
               (step s4 :logic "materialize mapped dispatch groups into task-runner manifest and task contracts"))
        :egress [context-pack.lisp dispatchable_groups accepted_shards task-runner-manifest task-contracts])
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
        :core ((step s1 :logic "resolve project/global memory scope and normalize KB or intent query")
               (step s2 :logic "read or mutate durable knowledge rows through one Lisp-described memory contract")
               (step s3 :logic "project search, beacon, insight, and memory responses into reviewable evidence"))
        :egress [kb_result memory_projection search_hits insight_summary])
      (function project-registry
        :surface project-registry
        :entry [mission_project ProjectRegistry.resolve]
        :core ((step s1 :logic "resolve registered project root from explicit project_id, cwd, or target_project")
               (step s2 :logic "reject ambiguous or outside-root runtime paths before workstation spawn")
               (step s3 :logic "return project metadata usable by request, plan, context, and workstation surfaces"))
        :egress [project_root project_id requested_cwd_policy]))

    (pillar communication
      (function conversation-ingestion
        :surface conversation-ingestion
        :entry [mission_conversation_query mission_conversation_analyze mission_conversation_reconcile mission_timeline mission_retrospective_manage mission_embedding_ops]
        :core ((step s1 :logic "ingest or query conversation/session/timeline records by project scope")
               (step s2 :logic "derive analysis, reconciliation, retrospective, and embedding work items")
               (step s3 :logic "surface durable facts for context assembly and later memory projection"))
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
        :core ((step s1 :logic "record capability usage, audit facts, and Codex operation acknowledgements")
               (step s2 :logic "bind evidence to plan/execution ids without becoming the primary execution gate")
               (step s3 :logic "return traceable receipts for later learning and report finalization"))
        :egress [capability_receipt audit_record codex_ops_result]))

    (pillar worker-runtime
      (function compute-primitives
        :surface compute-primitives
        :entry [mission_task_submit mission_task_query mission_task_cancel mission_job_poll mission_flow_run mission_pty_spawn mission_pty_send mission_pty_read mission_pty_signal mission_pty_confirm mission_pty_status mission_pty_screenshot mission_slots mission_slot_history mission_agent mission_inbox mission_sonnet_process mission_minimax_process mission_cc_query mission_cc_swarm mission_worker mission_control mission_pause mission_forge_build mission_forge_lint]
        :core ((step s1 :logic "normalize low-level runtime requests into slot, job, task, PTY, flow, forge, or process operations")
               (step s2 :logic "apply project-root, permission, timeout, and pause/control policies before side effects")
               (step s3 :logic "return durable runtime handles and status without bypassing BoardTask or plan execution when a higher-level surface exists"))
        :egress [runtime_handle job_status pty_snapshot flow_result forge_result])
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
               (step s2 :logic "enforce explicit side-effect policy and keep operational state separate from request artifacts")
               (step s3 :logic "return bounded operational status or mutation receipt"))
        :egress [infra_result permission_receipt daemon_update_status global_instruction_state])
      (function ops-infra
        :surface ops-infra
        :entry [scripts/deploy-daemon.sh scripts/cargo-fmt-touched.sh]
        :core ((step s1 :logic "build, backup, codesign, install, kickstart, and smoke daemon as one command")
               (step s2 :logic "retry IPC initialize smoke after socket readiness and rollback on real failure")
               (step s3 :logic "format only Rust files touched in current diff with rustfmt skip_children"))
        :egress [deployed-daemon rollback-result scoped-rustfmt-result])))

  (implementation-map
    (surface mission_request
      :status "code-aligned"
      :role "single user-facing request entry"
      :code ["crates/missiond-daemon/src/handlers/knowledge/request.rs"
             "crates/missiond-daemon/src/handlers/knowledge/request/request_artifacts.rs"
             "crates/missiond-mcp/src/tools/knowledge/request.rs"]
      :note "V3 physical split: request.rs remains the mission_request facade/review-packet/respond adapter, while request/request_artifacts.rs owns request-local paths, request.lisp and lifecycle event rendering, projection planning, pipeline-meta extraction, compat opt-in policy helpers, and JSON artifact status projection. v0 request-local projections: writes request.lisp + initial lifecycle event, runs unified_entry, then projects compiled_sexp / compiled_sexp_preview into .missiond/requests/<request_id>/{intent-alignment,plan}.lisp via atomic_write_artifact and surfaces a projection status (written|skipped_*|write_failed); status action exposes artifact paths + existence booleans; review_packet (state, artifact_kind, artifact_path, artifact_exists, artifact_preview, prompt, allowed_responses, next_action, execute_allowed) is derived from request-local artifact existence + latest projection + latest review event per the unified-entry/review-packet contract — UTF-8-safe via missiond_core::util::safe_byte_truncate; respond action accepts approve_intent/reject_intent/ask_question/approve_plan/reject_plan/execute_plan, resolves directive/plan refs from explicit args, request-local intent-alignment.lisp/plan.lisp parses, or prior request-local review events; approve_intent can create a hidden BoardTask anchor before s4 plan-authoring so callers do not need to know internal board ids; approve_plan can materialize request-local plan.lisp into a persisted draft Plan row, reusing plan.lisp's BoardTask anchor when present and creating a hidden anchor only if needed, then amends request-local plan.lisp with :plan_id + :version + :board_task_id before delegating to mission_plan approve; records a request-local review event under events/<seq>.event.lisp via the same atomic_write_artifact + monotonically-increasing local sequence; delegates approve/execute decisions to mission_directive / mission_plan / unified_entry without bypassing their gates, and returns blocked responses (with next_action) when refs are missing or execute=true was not passed; approve_intent is the unified-entry bridge for the human yes step: after directive approval succeeds it immediately calls unified_entry s4 plan-authoring and projects request-local plan.lisp so the next packet asks for plan review rather than requiring a separate advance call; approve_plan moves the packet to awaiting_execution so the next legal response is execute_plan; still no DB schema migration, no auto-approval, no direct workstation dispatch. Compat-writer switch: a single compat_write_requested boolean is derived from compat_write_file (V3 name) OR legacy write_file (alias) on the caller args; both keys are then stripped from the args forwarded to mission_directive / mission_plan, and write_file=true is re-injected only when compat_write_requested is true. Default mission_request flow therefore writes ONLY request-local artifacts under .missiond/requests/<request_id>/; the compat paths .missiond/alignment/<topic>/intent-alignment.lisp and .missiond/plans/<plan_id>/PLAN.lisp are opt-in legacy projections per the (compat-writer-policy ...) contract.")

    (surface unified-entry-runtime
      :status "code-aligned"
      :implements [unified-entry-pipeline request-runtime-bridge]
      :code ["crates/missiond-daemon/src/handlers/knowledge/unified_entry.rs"
             "crates/missiond-daemon/src/handlers/knowledge/unified_entry/planner.rs"
             "crates/missiond-daemon/src/handlers/knowledge/unified_entry/decorator.rs"
             "crates/missiond-daemon/src/handlers/knowledge/unified_entry/stages.rs"
             "crates/missiond-daemon/src/handlers/knowledge/request.rs"
             "scripts/check-v3-unified-entry-isomorphism.mjs"]
      :note "unified-entry-runtime is the daemon-local substrate for F-intent-alignment-plan-execution-loop; mission_request is the user-facing review-packet/respond adapter, while unified_entry.rs is now a thin staged-runtime facade. The V3 physical split is explicit: stages.rs owns FLOW_REF plus s1_message_intake, s3_alignment_review_gate, s4_plan_authoring, s5_plan_review_gate, and s6_execution_runner; planner.rs owns plan_pipeline plus the pure directive/plan/execute argument builders; decorator.rs owns ArtifactScope, build_artifact_refs, decorate, and planner-error envelope projection. run_pipeline dispatches to run_directive_compile_stage, run_plan_compile_stage, and run_plan_execute_stage, then decorate stamps pipeline_stage, flow_ref, artifact_refs, and next_step on responses. ArtifactScope projects Directive, Plan, and Execution artifact refs so mission_request can show request-local intent-alignment.lisp / plan.lisp state without reimplementing the routing. This surface composes mission_request, mission_directive, mission_plan, and mission_workflow into the same Lisp flow instead of letting each caller invent an independent request/directive/plan/workflow path.")

    (surface file-artifacts
      :status "code-aligned"
      :implements [file-artifacts request-local-artifacts compat-artifact-paths]
      :code ["crates/missiond-daemon/src/handlers/knowledge/file_artifacts.rs"
             "scripts/check-v3-file-artifacts-isomorphism.mjs"]
      :note "file-artifacts is the shared writer layer for file-first Lisp artifacts. ArtifactKind covers IntentAlignment, Plan, and Workflow; artifact_path maps stable compat path roots .missiond/alignment, .missiond/plans, and .missiond/workflows, while mission_request and task-runner surfaces layer request-local artifact projection under .missiond/requests/<request_id>/ on top. atomic_write_artifact and unique_temp_path_in_dir provide the temp-file + fsync + rename discipline; attempt_artifact_write with WriterContext returns AttemptOutcome::Written, ResolveFailed, or WriteFailed so DB/file partial states stay visible. The invariant is no partial writes: failed writes must not leak partial bytes, and callers must surface write_failed / partial status rather than pretending the Lisp artifact is authoritative.")

    (surface mission_directive
      :status "code-aligned"
      :implements [intent-alignment alignment-review-gate]
      :code ["crates/missiond-daemon/src/handlers/knowledge/directive.rs"
             "crates/missiond-mcp/src/tools/knowledge/directive.rs"]
      :note "dry_run emits a deterministic directive-draft Lisp artifact with utterance/source/status; sonnet output is accepted only when it is one balanced Lisp s-expression with head directive|directive-draft|intent-alignment. Persisted directive Lisp is enriched with :directive_id + :version before being surfaced as compiled_sexp(_preview) and before optional file-first writes. The compatibility file writer targets ArtifactKind::IntentAlignment at .missiond/alignment/<topic>/intent-alignment.lisp, never rolls back a committed row on file failure, and review_gate_policy only emits/records gates; it never auto-approves intent.")

    (surface mission_plan
      :status "code-aligned"
      :implements [plan plan-review-gate plan-runner evidence-collector]
      :code ["crates/missiond-daemon/src/handlers/knowledge/plan.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan/compile_authoring.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan/approval_review.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan/field_inference.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan/execution_runtime.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan/internal_dispatch.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan/execute_hints.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan/task_contract.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan/distill_chain.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan/dispatch_response.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan/evidence_sidecar.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan/router_policy_dry_run.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan/task_runner_dry_run.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan/tests.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan_dag.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan_dag/parser.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan_dag/acceptance.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan_dag/claim_lease.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan_dag/rollback.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan_dag/resume.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan_dag/projection.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan_dag/finalization.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan_dag/lifecycle.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan_dag/scheduler.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan_dag/mode.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan_dag/tests.rs"
             "crates/missiond-mcp/src/tools/knowledge/plan.rs"]
      :note "compiler_mode=dry_run now renders plan-draft as an executable Lisp scaffold with :target, :objective, and :nodes; execute can derive target_source=plan_hint from plan.sexp_text instead of caller escape parameters. plan/compile_authoring.rs owns mission_plan plan-authoring entry/core: action_compile, action_compile_dry_run, action_compile_sonnet, render_dry_run_plan_sexp, validate_compiled_plan_sexp, build_planner_system_prompt, build_planner_user_prompt, and maybe_write_plan_artifact project compiler_mode dry_run/sonnet into persisted plan rows and optional PLAN.lisp file artifacts. plan/approval_review.rs owns mission_plan plan-review-gate entry/core/egress: action_approve, action_mark, action_supersede, review_automation_policy handling, LLM auto-approve propose-only audit blocks, maybe_emit_review_question_resolved, PlanSubscriberOutcome, and handle_review_resolved_event keep approval/rejection/needs_changes transitions tied to the same review envelope validation. plan/field_inference.rs owns mission_plan execute preflight field inference/core: infer_plan_fields, workstation_inference_mode, PlanFieldInference, LLM proposal blocks, apply_gate, persisted_apply, and safe apply/preview response projection before execution. plan/execution_runtime.rs owns mission_plan execute entry/core/egress orchestration: action_execute preflight, preview/apply branch selection, paused-node and DAG handoff, workstation proposal/auto-spawn gate, action_execute_bridge, and action_execute_internal dispatch while preserving the plan.rs facade. plan/internal_dispatch.rs owns mission_plan inner target argument projection: build_internal_dispatch_args maps Lisp hints and caller args into mission_execution, mission_task_delegate, and mission_flow_run payloads; derive_objective_from_plan/truncate_chars cap delegated objectives; tool_result_payload preserves downstream JSON payload compatibility for plan_dag, workflow, and workstation dispatch. plan/execute_hints.rs owns mission_plan PLAN.lisp hint parsing: ParsedPlanHints, ResolvedExec, parse_plan_hints, scan_keyword_pairs, split_lisp_string_list, normalize_target, canonicalize_strategy, and resolve_dispatch_strategy are the execution-hint boundary shared by mission_plan, plan_dag, and workstation-dispatch brief projection. plan/task_contract.rs owns mission_plan task-contract Lisp projection: TaskContractEmitMode, DispatchContractMode, TaskContractInputs, parse_task_contract_emit_mode, parse_dispatch_contract_mode, build_task_contract_lisp, write_task_contract_under_root, emit_task_contract, task_contract_inputs_from_hints, and task_contract_inputs_from_hints_with_trace produce the generated .missiond/tasks/generated/<plan>/<node>.lisp contract and the optional :session-trace-path projection before workstation dispatch. plan/distill_chain.rs owns mission_plan cross-plan distill-chain egress: parse_distill_chain_mode, validate_distill_chain_args, apply_distill_chain, build_distill_chain_block, and attach_distill_chain_to_payload project finalize-plan success into a typed distill_chain response/evidence record without changing the underlying plan finalization. plan/dispatch_response.rs owns mission_plan execution response egress: validate_session_trace_path_arg, attach_session_trace_response_fields, merge_task_contract_block, build_task_contract_failure_response, build_task_contract_dry_run_response, build_workstation_dispatch_response, and build_internal_dispatch_success_response assemble session-trace, task-contract, workstation-dispatch, and internal-dispatch status fields without changing the core dispatch path. plan/evidence_sidecar.rs owns mission_plan evidence sidecar egress: action_record_evidence and append_plan_evidence_entry persist manual evidence and plan-runner dispatch audit entries under .missiond/v2/plans with typed evidence_collector stamps when evidence_kind/source are supplied. plan/router_policy_dry_run.rs owns the mission_plan router-policy adapter: parse_router_policy_mode and attach_router_recommendation_block project router-policy / trace-index / backend-readiness / dispatch-descriptor Lisp facts into an advisory router_recommendation block while keeping applied=false, no runtime replacement, and byte-identical off mode. plan/task_runner_dry_run.rs owns the mission_plan task-runner adapter: parse_task_runner_mode and attach_task_runner_block project a task-runner manifest into the advisory task_runner response block while keeping applied=false, no worker spawn, and byte-identical off mode. plan/tests.rs holds the historical mission_plan regression suite outside the runtime facade so plan.rs remains a small action router and module boundary. plan_dag/parser.rs owns the DAG parser/validator core: DagNode, ParsedDag, DagBuildError, review/retry constants, node form scanning, keyword pair parsing, topological sort, and validate-before-dispatch invariants live outside the runtime loop. plan_dag/acceptance.rs owns the DAG acceptance core: acceptance-mode, acceptance-requires, status projection, inner-payload signal checks, fan-in evaluation, and deterministic acceptance pause ids. plan_dag/claim_lease.rs owns the DAG claim/lease core: claim_lease_secs, claimer_name, enforce_claims, scope derivation, deterministic claim ids, ClaimRegistry conflict detection, and planned claim projection. plan_dag/rollback.rs owns the DAG rollback/cascade core: rollback policy/cascade parsing on DagNode, descriptor safety checks, rollback evaluation/status projection, compensation ordering, cascade dispatch-safe mapping, and rollback brief preview truncation. plan_dag/resume.rs owns the DAG review-resume entry/egress core: PlanNodeResumeError, validate_resume_request, action_execute_resume, resume decision evidence, PlanNodeResumeListenerOutcome, and handle_review_resolved_plan_node_event keep explicit execute resumes and bus-resolved plan-node resumes on the same validator/dispatch path. plan_dag/projection.rs owns the DAG response projection core: node summaries, retry plan projection, and unsupported hint summary preserve dry-run/live response shape outside the runtime loop. plan_dag/finalization.rs owns the DAG finalization projection core: finalize_plan and distill_on_success parsing, plan status label mapping, finalization response blocks, and distill response blocks stay pure outside the scheduler loop. plan_dag/lifecycle.rs owns the DAG lifecycle event/evidence projection core: EvidenceCtx, deterministic PlanNodeStateChanged ids, bus publish fallback, dag_finalized rows, running/finished/skipped/paused/claimed/released/conflict evidence rows, and retry predicate stay outside the scheduler loop. plan_dag/scheduler.rs owns the DAG scheduler projection core: dry-run concurrency wave projection, max_parallel_nodes parsing, taint propagation, and per-node inner argument projection stay outside the runtime loop. plan_dag/mode.rs owns the DAG scheduler-mode gate: scheduler_mode detection and DAG-mode LLM inference refusal stay as the facade-level handoff guard before runtime dispatch. plan_dag/tests.rs does the same for the DAG scheduler regression suite while plan_dag.rs continues to own the DAG runtime. DAG execution parses node-local Lisp hints (:target, :objective, :timeout-ms, :target-project, :requested-cwd, :acceptance-commands, :workstation-dispatch) and forwards them into the same internal dispatch path. unified_entry owns only routing/argument forwarding: s4 compile forwards target/objective/project hints into mission_plan, and s6 execute forwards approved_plan_id + execute=true knobs without inventing a second plan schema.")

    (surface evidence-collector
      :status "code-aligned"
      :implements [verification-receipt]
      :code ["crates/missiond-daemon/src/handlers/knowledge/evidence_collector.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan/execution_runtime.rs"
             "scripts/check-v3-evidence-collector-isomorphism.mjs"]
      :note "EVIDENCE_SCHEMA_VERSION pins the evidence wire shape for verification-receipt consumers. EventRefStatus is the closed status enum live | log | unavailable describing whether an event ref is live from publish, recovered from the event log, or unavailable. EventRefProvenance is the closed provenance enum live | passive_cache | event_log_query | unavailable so consumers can distinguish immediate publish results, the bounded in-memory passive cache, and the bounded event_log_query recovery path. EVENT_REF_CACHE_CAP = 1024 fixes the passive cache capacity. wrap_legacy_record_evidence lifts caller-supplied JSON evidence into the typed EvidenceEntry envelope without losing prior fields, keeping plan.rs compatibility while making the receipt payload Lisp-addressable. plan/execution_runtime.rs owns the mission_plan internal dispatch evidence append path through evidence_collector::append, while plan/evidence_sidecar.rs owns manual record_evidence compatibility wrapping.")

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
	      :note "mission_execution-log is the durable companion-log and live-projection surface for mission_execution. agent_execution/lisp_syntax.rs owns the shared S-expression parser and compatibility re-exports. agent_execution/lisp_syntax_node.rs owns Node/NodeKind and node accessors. agent_execution/lisp_syntax_balance.rs owns check_balance delimiter audit. agent_execution/log_paths.rs owns COMPANION_DIR .missiond/v2, resolve_project_root, companion_path, project_or_target_project, and require_str for companion-log ingress path resolution. agent_execution/log_store.rs owns LogFile/read_log_file/write_log_file, parse_kv_pairs, list_block_summaries, json_strip_quotes, and read-model helpers so the Lisp companion log stays authoritative. agent_execution/log_mutation.rs owns Lisp quoting, locate_kv_value, update_kv_in_node, append_to_block, touch_last_updated, and refresh_root after durable companion-log edits. agent_execution/log_template.rs owns render_canonical_template for canonical companion-log Lisp projection. agent_execution/log_dispatch.rs owns VALID_DISPATCH_STRATEGIES, DEFAULT_DISPATCH_STRATEGY, normalize_dispatch_strategy, build_opened_event, DispatchMeta, and read_dispatch_metadata_from_log. agent_execution/log_counters.rs owns ID counters, allocate_id, scan_max_id, and insert_id_counters_block for claims/governance/completion writes and repair backfills. The facade agent_execution.rs keeps the public MCP action router. agent_execution/log_open.rs owns action_open and session-trace dispatch projection. agent_execution/log_list.rs owns action_list and compact execution durability rows. agent_execution/log_surface.rs keeps emit_execution_event plus compatibility re-exports after durable writes succeed. agent_execution/log_governance.rs is the governance facade. agent_execution/log_deviation.rs owns action_deviate and DeviationRecorded projection. agent_execution/log_decision.rs owns action_decide and DecisionRecorded projection. agent_execution/log_issue.rs owns action_issue and IssueRecorded projection. agent_execution/log_status.rs owns action_status plus the status read-model projection for active claims, open issues, unresolved deviations, latest decisions, completed_phases, and durability. agent_execution/session_trace_event.rs owns TraceKind, TraceEvent, TraceWarning, TRACE_ID_RE, render_trace_event, scan_max_trace_seq, sanitize_trace_backend, and is_valid_trace_id. agent_execution/session_trace.rs owns append_session_trace_event, resolve_session_trace_path, and resolve_trace_task_id for the optional task session-trace projection used by open/preflight_commit/complete; trace write failures surface trace_warning and never abort the primary action.")

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
	      :note "mission_execution-completion-audit owns the completion durability gate. action_complete records completion facts from the facade, while agent_execution/completion_fields.rs owns VALID_COMMIT_STATUSES, VALID_VERIFIER_STATUSES, VALID_TASK_RUN_VERIFIER_STATUSES, normalize_commit_status, normalize_verifier_status, normalize_task_run_verifier_status, collect_string_list, render_string_list, parse_string_list, and the commit-status-without-hash / commit-status-blocked-without-blocker / scoped-commit-violation finding constants. agent_execution/completion_inputs.rs owns CompletionRequest, parse_completion_request, parse_commit_status, parse_verifier_status, parse_task_run_verifier_status, and trimmed_string_arg for completion action ingress normalization. agent_execution/completion_records.rs owns CompletionRecord and parse_completions. agent_execution/completion_durability.rs owns summarize_durability and canonical_status_str for dashboard durability projection. agent_execution/completion_audit.rs owns action_complete only. agent_execution/completion_entry.rs owns CompletionEntryFields and render_completion_entry for companion-log Lisp entry projection. agent_execution/completion_response.rs owns CompletionResponseFields and build_completion_response for JSON egress projection. agent_execution/completion_id_audit.rs owns check_id_monotonic duplicate-id audit. agent_execution/completion_handoff_audit.rs owns audit_scoped_commit_handoff for scoped commit handoff findings. agent_execution/completion_contract_gate.rs owns enforce_task_contract_completion for task-contract evidence gates. agent_execution/completion_gates.rs owns enforce_scoped_commit_completion and compatibility re-exports for split completion gates. agent_execution/completion_indexes.rs owns rebuild_derived_indexes for durable-slot-derived cache reconstruction. agent_execution/completion_maintenance.rs owns action_audit, ExecutionEvent::Audited, and ExecutionEvent::StaleClaim. agent_execution/completion_audit_findings.rs owns AuditFindings, collect_audit_findings, paren/id/claim/issue/stale/scoped-commit findings, and parse-failed response projection. agent_execution/completion_repair.rs owns action_repair, ExecutionEvent::Repaired, id-counter synthesis, stale-claim marking, and derived-index rebuild apply/dry-run repair. agent_execution/completion_trace.rs owns append_completion_trace_if_requested and the complete/failure session-trace projection for completion responses. agent_execution/completion_verification.rs owns CompletionVerificationOutcome and evaluate_completion_verification for daemon-auto-verifier versus legacy-caller-claim decisioning. action_audit reports findings, and action_repair marks stale claims. agent_execution/task_verifier_inputs.rs owns ReportSummary, SharedMemorySummary, read_report_summary, read_task_contract_id, read_shared_memory_ledger, and read_completion_task_id for report-contract/task-contract/shared-memory Lisp projection. agent_execution/task_verifier_preconditions.rs owns VerifiedCompletionInputs and verified=true required-field enforcement. agent_execution/task_verifier_report.rs owns verify_report_against_contract plus report schema/task_id/commit_hash alignment errors shared by legacy and auto verifier gates. agent_execution/task_verifier_auto_artifacts.rs owns resolve_verifier_artifact_path, read_task_contract_artifact, read_report_artifact, and read_shared_memory_artifact for verifier IO and error projection. agent_execution/task_verifier_auto.rs owns auto_run_task_run_verifier for the in-process task-run verifier over task-contract/report/shared-memory artifacts. agent_execution/task_verifier.rs owns enforce_verified_completion for the legacy verified=true gate. agent_execution/preflight.rs owns preflight_commit action wiring before a writer commits. agent_execution/preflight_cwd.rs owns resolve_preflight_inspect_dir, cwd canonicalization, and project-boundary rejection. agent_execution/preflight_contract.rs owns apply_task_contract_projection, task_contract_status, staged_out_of_scope, staged_forbidden, unstaged_in_scope, and task_contract_scope promotion. agent_execution/preflight_contract_scope.rs owns build_contract_scope_summary, evaluate_task_contract_for_preflight, load_task_contract integration, and write-scope / must-not-touch projection. agent_execution/preflight_trace.rs owns append_preflight_trace_if_requested and the preflight observation session-trace projection. agent_execution/preflight_patterns.rs owns pattern_matches_path plus repo-relative glob normalization. agent_execution/preflight_porcelain.rs owns PorcelainEntry, parse_porcelain_status, and read-only git status. agent_execution/preflight_scope.rs owns build_preflight_summary and claim-scope projection.")

    (surface mission_workflow
      :status "code-aligned"
      :implements [workflow workflow-distiller]
      :code ["crates/missiond-daemon/src/handlers/knowledge/workflow.rs"
             "crates/missiond-daemon/src/handlers/knowledge/workflow/artifacts.rs"
             "crates/missiond-daemon/src/handlers/knowledge/workflow/methodology.rs"
             "crates/missiond-daemon/src/handlers/knowledge/workflow/review_resolution.rs"
             "crates/missiond-mcp/src/tools/knowledge/workflow.rs"]
      :note "workflow.rs remains the mission_workflow action facade, workflow distill/review glue, and Sonnet/auto-chain policy adapter; workflow/artifacts.rs owns workflow file-first writer args, best-effort ArtifactKind::Workflow writes, V3 workflow S-expression rendering, methodology match_rules projection, and JSON-to-Lisp egress helpers; workflow/methodology.rs owns the deterministic methodology compiler sub-surface: methodology path resolution, Lisp source validation, step/higher-order form lifting, GeneratedMeta, executable YAML rendering, atomic generated-YAML write, and compiled-flow resolution for run_methodology; workflow/review_resolution.rs owns resolve_review ingress, WORKFLOW_REVIEW_ACTIONS, WORKFLOW_REVIEW_VERSION, persisted-vs-methodology receipt routing, policy-only automation, WorkflowSubscriberOutcome, and the subscriber-side Resolved-event bridge. distill dry_run emits workflow-draft Lisp; sonnet distiller requires JSON workflow_sexp + object match_rules and validates balanced workflow_sexp before persisting a Workflow row. distill persist+write_file writes an enriched V3 workflow artifact with :workflow_id, :source_plans, :match_rules, :steps, :status, and :body workflow_sexp under ArtifactKind::Workflow at .missiond/workflows/<topic>.lisp, preserving partial-on-file-failure semantics. compile_methodology reads methodology Lisp from .missiond/workflows/<name>.lisp or workflow_path, then deterministic mode validates Lisp and emits executable YAML through the same workflow surface; its persist+write_file path now also projects the methodology compile through render_workflow_artifact_sexp so .missiond/workflows/<topic>.lisp is the same enriched V3 workflow artifact shape — :workflow_id stamped with the generated methodology flow_id, :source_plans [], :match_rules carrying source_kind=methodology / compiler / compiler_version / source_hash / flow_id / source_path / generated_at, :steps extracted from the methodology body, :status compiled (or compiled_review_required when no steps), and :body containing the methodology Lisp body — instead of canonicalizing the raw methodology source. The methodology branch still has no Workflow DB row (no workflow_id UUID, no schema migration); only the file projection is upgraded so reviewers see the same Lisp truth shape as distill. run_methodology dispatches that compiled YAML. Review gates remain receipt-only, and auto_sonnet_policy={off|safe_after_rules|dry_run} is a closed enum.")

    (surface review-gate
      :status "code-aligned"
      :implements [alignment-review-gate plan-review-gate workflow-review-gate two-gate-default]
      :code ["crates/missiond-daemon/src/handlers/knowledge/review_gate.rs"
             "crates/missiond-daemon/src/handlers/knowledge/directive.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan/compile_authoring.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan/approval_review.rs"
             "crates/missiond-daemon/src/handlers/knowledge/workflow.rs"
             "crates/missiond-mcp/src/tools/knowledge/directive.rs"
             "crates/missiond-mcp/src/tools/knowledge/plan.rs"
             "crates/missiond-mcp/src/tools/knowledge/workflow.rs"
             "scripts/check-v3-review-gate-isomorphism.mjs"]
      :note "review-gate is the shared event-bus review layer behind alignment-review-gate, plan-review-gate, workflow review, and the V3 two-gate-default axiom. review_gate.rs owns ReviewGatePolicy manual | emit_question | off and ReviewDecision approved | rejected | needs_changes; parse_review_gate_policy / parse_compile_review_gate normalize caller inputs, apply_compile_review_gates fans out to maybe_emit_review_question_created or auto_emit_review_question_after_artifact_write, and maybe_emit_review_question_resolved records explicit review decisions. The gate must never auto-approve intent or plan, never wait for a human in the primary compile/approve path, and never roll back a committed DB/file artifact because the bus failed; failures surface review_question_warning with deterministic review_question_id so callers can retry or resolve manually. directive.rs, plan/compile_authoring.rs, plan/approval_review.rs, plan.rs, and workflow.rs are the only callers for these gate helpers and their MCP schemas expose review_gate_policy plus review_question_id / review_decision so the Lisp-level review packet can stay a pure request-local projection.")

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
             "scripts/check-verification-receipt.mjs"
             "scripts/verify-task-contract.mjs"
             "scripts/verify-task-runner-batch.mjs"]
      :note "Task-scoped lifecycle events are first-class one-event files: the primary task-scoped path is .missiond/tasks/<wave>/events/<seq>.event.lisp (one lifecycle-event form per file, schema=missiond.task-lifecycle-event.v1, validated by check-task-lifecycle-events as standalone task-scoped event files), and task-runner-append-event allocates the next numeric file under a directory lock, validates the candidate bytes, and atomically creates them via fs.openSync(file, 'wx') when --events-dir is supplied. The legacy task-scoped task-lifecycle-events.lisp ledger is now a compatibility projection/input only: existing --ledger callers keep working unchanged, and task-runner-wave-state reads conventional task-scoped event files when present and falls back to the legacy ledger for historical waves, deduping by event id when both inputs exist. task-runner-append-event can ALSO project each append into request-local one-event files at .missiond/requests/<request_id>/events/<seq>.event.lisp when request-id/request-events-dir are supplied; task-runner-dispatch and task-runner-submit-dispatch pass both task-scoped events-dir and request-local lifecycle projection args through their real dispatch-event paths, via task-runner-next-action, instead of leaving them as append-helper-only knobs. task-runner-dispatch projects manifest/action model_profile, timeout_secs, and context_pack_path into mission_task_delegate target_args, context_hints, and the worker objective; it defaults missing model_profile to coding-default-opus-4-7 and derives timeout_secs from estimated_minutes only when no explicit node timeout exists. plan/task_runner_dry_run.rs is the in-daemon task-runner adapter for mission_plan execute dry_run: parse_task_runner_mode validates off|dry_run up front and attach_task_runner_block projects manifest batches, critical path, tier counts, and overlap diagnostics as an advisory task_runner block with applied=false, no worker spawn, no git, no network, and no file I/O on off/default mode. task-runner-finalize-report can project the same finalized lineage into the V3 request-local final-report artifact at .missiond/requests/<request_id>/reports/final.lisp when request-id/request-reports-dir are supplied; the legacy report-contract remains the compatibility report. check-verification-receipt can project a single receipt into the V3 request-local verification-receipt artifact at .missiond/requests/<request_id>/receipts/<receipt_id>.lisp when request-id/request-receipts-dir are supplied through renderRequestVerificationReceipt + validateRequestVerificationReceiptSource + writeRequestVerificationReceiptFile; the writer rejects malformed request_id, malformed receipt_id, absolute or .. paths inside the resolved target, and invalid receipt objects (validateReceiptObject), validates the rendered Lisp bytes through the existing structural checker before atomic create-only rename, and refuses to overwrite an unrelated receipt file unless mode=replace and the on-disk bytes already equal the candidate; legacy task-scoped (verification-receipt-set ...) Lisp files remain compatibility inputs for readVerificationReceiptFile and verify-task-runner-batch --receipts. task-runner-append-event is the only cooperative mutation helper for task-lifecycle-event-log and task-scoped event files: it uses a sibling lock, rereads under lock, validates the candidate ledger or standalone event bytes, then atomically renames/creates outputs. task-runner-next-action prioritizes finalize_report before dispatch_task and emits dispatch events through appendLifecycleEvent. task-runner-parent-hotfix is read-only by default and projects parent patches through task-runner-finalize-report, preserving worker commit as :agent_commit_hash while :commit_hash/:final_commit_hash/:verified_commit_hash move to the final commit, and can also write the V3 request-local final-report projection. parent-hotfix finalization is a sparse Lisp projection over the worker report: task-runner-finalize-report parses the worker report's keyword/value pairs and re-emits them as-is, patching only the lineage fields (:status :commit_hash :agent_commit_hash :final_commit_hash :verified_commit_hash :parent_patches plus the unioned :files_changed) while preserving optional report-contract fields the worker already wrote (:notes :verification_tier :time_sinks :major_decisions :unexpected_work :blockers :trace_refs router-recommendation / router-readiness / router-dispatch-descriptor / verification-receipts and any additive optional fields). :acceptance_results is preserved by default; --acceptance-command appends new entries rather than replacing the worker block unless an explicit replacement opt is supplied. project-task-lifecycle-ledger backfills shared-memory/session-trace compatibility facts from lifecycle events. verify-task-runner-batch imports lifecycle validation, append-event, and parent-hotfix projections so batch smoke covers the task-scoped event files, the legacy ledger compatibility, the final-report, and the receipt path. verify-task-contract is the commit-snapshot artifact validator: real --commit verification stays read-only on git, but discovers known Lisp artifact paths (.missiond/tasks/<wave>/session-trace.lisp -> check-session-trace, .missiond/tasks/<wave>/shared-memory.lisp -> check-task-memory, .missiond/tasks/<wave>/task-lifecycle-events.lisp and .missiond/tasks/<wave>/events/*.event.lisp -> check-task-lifecycle-events, .missiond/tasks/<wave>/reports/*.report.lisp -> check-task-report) from the contract :write-scope union with the resolved commit's modified files via planArtifactValidation, materializes each artifact's commit bytes into a temp tree using git show <commit>:<path> through validateCommitArtifacts, and runs the existing checker scripts against the materialized bytes so a worker-commit defect (for example a wave51 session-trace with :kind acceptance) cannot pass even after a later parent hotfix repairs the working tree. The pure verifyContract(contract, commitInfo) API stays unchanged for verify-task-run and verify-task-runner-batch importers; artifact validation runs only on the verify-task-contract CLI path, with --dry-fixture covering the planArtifactValidation cases and a self-contained invalid session-trace bytes regression that exercises the spawn-checker code path without git access.")

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

    (surface context-pack
      :status "code-aligned"
      :implements [multi-agent-context-pack]
      :code ["scripts/check-context-pack.mjs"
             "scripts/context-pack-append.mjs"
             "scripts/context-pack-compile-shards.mjs"
             "scripts/context-pack-materialize-wave.mjs"
             "scripts/check-v3-context-pack-isomorphism.mjs"]
      :note "Context-pack is the V3 high-density planning surface for two-stage parallel work: context investigators append claim/observation/anchor/shard-proposal/conflict entries to .missiond/tasks/<wave>/context-pack.lisp without code edits, then an orchestrator/integrator appends integration-plan with accepted-shards and dispatch-groups. Mapped dispatch groups use (group :id <id> :shards [...]) so scripts/context-pack-compile-shards.mjs can project the Lisp plan into dispatchable_groups for code workers; legacy bare group ids remain names_only for older packs. scripts/context-pack-materialize-wave.mjs is the next projection boundary: it refuses names_only groups, converts accepted mapped shards into a task-runner-manifest.v2 plus one task-contract.v1 per shard, preserves model_profile/timeout/context_pack_path in manifest nodes, and leaves actual dispatch to prepare-task-runner-wave + task-runner-dispatch/submit. The structure deliberately mirrors the proven shared-memory append-only pattern but raises the semantics from lifecycle notes to implementable shard planning. scripts/context-pack-append.mjs is the cooperative mutation path: it creates a missing pack when wave/purpose are supplied, takes a sibling lock, allocates the next :seq, injects :at, validates candidate bytes, and atomically renames, including --dispatch-group-shards for mapped integration plans. scripts/check-context-pack.mjs validates missiond.context-pack.v1 headers, unique ids, strictly increasing seq, ISO timestamps, repo-relative paths, shard-proposal owner/write-scope/must-not-touch/acceptance, integration-plan accepted-shard references, mapped dispatch coverage, and accepted shard write-scope non-overlap. Code workers consume the finalized integration-plan and avoid re-deriving architecture; context investigators may run concurrently because they never rewrite prior entries.")

    (surface workstation-config
      :status "code-aligned"
      :implements [workstation-config]
      :code ["crates/missiond-daemon/src/handlers/compute/compute_slot.rs"
             "crates/missiond-daemon/src/handlers/compute/task_delegate.rs"
             "crates/missiond-daemon/src/context/slot_env.rs"
             "crates/missiond-daemon/src/slot_orchestrator/spawner.rs"
             "crates/missiond-daemon/src/engine/intent_engine/autopilot.rs"
             "crates/missiond-mcp/src/tools/compute/compute_slot.rs"
             "crates/missiond-mcp/src/tools/compute/task_delegate.rs"]
      :note "mission_compute_slot and mission_task_delegate accept model/model_profile; coder/researcher default to Claude Code Default(Opus 4.7/1M) by omitting --model. compute_slot objective is metadata only; direct warmup requires explicit initial_prompt, and delegated task_delegate auto-provision still carries suppress_initial_prompt=true. spawn_tracked_slot now syncs MissionD Claude hooks project-locally via slot_env::sync_slot_hooks_to_local_settings, preserving permissions and existing hooks while adding SessionStart session-register + UserPromptSubmit context-prefetch before PTY start; build_slot_tracking_env injects MISSION_IPC_ENDPOINT so hooks reconnect to the active daemon instead of relying on stale global defaults. Autopilot pty.send budget, smart-watchdog idle-recovery threshold, and Autopilot BoardTask claim lease are now projections of BoardTask.timeout_secs (default 1800s, clamp 60..7200, watchdog grace 120s); the no-PTY-session branch retains a 120s probe window for missing slot processes — see derive_pty_timeout_secs / idle_watchdog_threshold_secs / derive_board_task_lease_secs in autopilot.rs. The fixed 20-minute claim lease is gone; the lease now equals idle_watchdog_threshold_secs so the watchdog cannot reclaim a slot whose claim is still legitimately ticking inside its declared timeout. Autopilot prompt assembly projects the V3 prompt-tool-contract via build_base_prompt (objective dedupe) and append_board_task_id_suffix (conditional board self-close); the prompt no longer hardcodes mission_board_update / mission_board_note_add as unconditional must-calls. The V3 execution-ownership rule for delegated BoardTasks projects to: compute_slot::effective_initial_prompt + explicit initial_prompt + suppress_initial_prompt arg (delegated path starts the slot idle), task_delegate::auto_provision_slot create_args carrying suppress_initial_prompt=true, and autopilot dispatch_board_tasks holding an OwnedSlotDispatchGuard across state.pty.send + post-send tail inside a tokio::task::JoinSet send-task so different-slot sends run concurrently within a single dispatch tick while same-slot exclusion still covers the full close-owner / KB-feedback / deploy-review sequence, with decide_close_action preserving Done self-close and Blocked question states. Restart recovery clears stale slot-dyn-* BoardTask assignee pins via BoardStore::clear_board_task_assignee before normal no-assignee routing resumes.")

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
             "crates/missiond-daemon/src/handlers/knowledge/workstation_dispatch/tests.rs"
             "scripts/check-v3-request-flow-smoke.mjs"
             "scripts/check-v3-workstation-dispatch-isomorphism.mjs"]
      :note "workstation-dispatch is the substrate called by mission_plan execute_internal after target=mission_task_delegate is selected; workstation-config owns slot/model/prompt setup, while this surface owns the WorkstationDispatchOutcome response vocabulary and the handoff contract. workstation_dispatch/outcome.rs owns WorkstationDispatchOutcome, SafeDescriptorReason, outcome_to_response_fields, extract_inner_board_task_id, truncate_brief_preview, SCOPED_COMMIT_REQUIRED, and SCOPED_COMMIT_POLICY; WorkstationDispatchOutcome has DryRun and Dispatched terminal success branches plus InnerError and SafeDescriptor safety branches; outcome_to_response_fields projects runner_status=workstation_dispatch_v0, workstation_dispatch_status=dry_run_no_dispatch|dispatched|inner_returned_error, target_tool=mission_task_delegate, task_brief_preview, inner_result, and delegated_board_task_id. workstation_dispatch/runner.rs owns run_workstation_dispatch, run_workstation_dispatch_with_contract, run_workstation_dispatch_with_contract_and_trace, the resolve_target_project_root preflight, resolve_contract_path/load_task_contract overlay, build_task_brief_with_source_and_trace call, mission_task_delegate inner adapter, tool_result_payload conversion, task_contract_source_path/session_trace_path threading, and evidence_collector::append sidecar. workstation_dispatch/descriptor.rs owns the task-contract v1 Lisp projection boundary: ParsedTaskContract, TaskContractParseError, parse_task_contract, load_task_contract, and resolve_contract_path_public. workstation_dispatch/decision.rs owns the entry decision gate: WorkstationDispatchSource, DispatchDecision, InferenceContext, INFERABLE_DISPATCH_STRATEGIES, opt_in_requested, explicit_workstation_dispatch_flag, and evaluate_dispatch_decision. workstation_dispatch/brief.rs owns BriefTaskKind, classify_task_kind, build_task_brief, build_task_brief_with_source, and build_task_brief_with_source_and_trace, including the Completion handoff (scoped commit), Session trace block, COMMIT_POLICY_SCOPED default, and AGENT_TEAM_OBJECTIVE_HINT projection. workstation_dispatch/proposal.rs owns the propose-only LLM surface: WorkstationProposalBundle, WorkstationProposalGate, request_workstation_proposals, parse_workstation_proposals, safety/confidence/status enums, applied=false, auto_spawn=false, and no fallback to claude -p. workstation_dispatch/auto_spawn.rs owns the true-spawn gate: WorkstationAutoSpawnInput, WorkstationAutoSpawnGateOutcome, WorkstationAutoSpawnStatus, WorkstationProposalHashStatus, parse_workstation_auto_spawn_input, compute_workstation_proposal_hash, enforce_auto_spawn_preflight, and evaluate_workstation_auto_spawn_gate. workstation_dispatch/tests.rs holds the historical workstation-dispatch regression suite outside the facade, including opt-in decision, task-contract parsing, proposal parsing, auto-spawn hash/gate, and session-trace brief smokes. The --execute-dry-run audit must reach this substrate and return dry_run_no_dispatch rather than bridge_only; the --execute-real-dispatch audit must return dispatched with delegated_board_task_id without synchronously waiting for the worker.")

    (surface mission_board
      :status "code-aligned"
      :implements [mission-board board-task-lifecycle board-claim-lease]
      :code ["crates/missiond-daemon/src/handlers/knowledge/board.rs"
             "crates/missiond-core/src/types/board.rs"
             "crates/missiond-core/src/db/traits.rs"
             "crates/missiond-core/src/db/pg/board.rs"
             "crates/missiond-mcp/src/tools/knowledge/board.rs"
             "scripts/check-v3-board-isomorphism.mjs"]
      :note "mission_board is the durable BoardTask coordination surface underneath delegated ClaudeCode work: MCP exposes query/create/update/delete/claim/decompose/retry/note_add with a generated schema from .missiond/intent-tools.lisp, while the daemon handler records session-task bindings, publishes BoardEvent created/updated/status_changed facts, and routes create/update/claim/note/retry operations through BoardStore. BoardTaskStatus is the closed persisted status vocabulary (open/running/verifying/done/blocked/failed/skipped); BoardTask carries assignee, auto_execute, depends_on, claim_executor_id, claim_executor_type, claimed_at, lease_expires_at, timeout_secs, and notes_count so Autopilot, workstation dispatch, and humans observe the same row. BoardStore is the single trait boundary for BoardTask CRUD, atomic claim_board_task, open-only clear_board_task_assignee, release_board_claims_by_executor, recover_stale_running_tasks, clear_dangling_dynamic_slot_assignees, set_board_task_lease, list_autopilot_tasks, dependency checks, retry, notes, and BoardTask-with-context queries. PgMissionStore pins the concurrency semantics: claim_board_task updates only rows with status='open' and claim_executor_id IS NULL, release only resets running rows for the executor, clear_board_task_assignee only clears the expected assignee while the task is still open, stale recovery honors lease_expires_at first and falls back to timeout_secs, clear_dangling_dynamic_slot_assignees targets only assignee LIKE 'slot-dyn-%' with no active dynamic slot, and list_autopilot_tasks orders assigned tasks first before order_idx. This surface is intentionally separate from workstation-config: workstation-config owns slot/model/prompt dispatch, mission_board owns the durable BoardTask row and claim/lease/retry/note semantics those dispatchers mutate.")

    (surface memory-kb
      :status "designed"
      :implements [knowledge-memory kb-manager memory insight intent-snapshot]
      :code ["crates/missiond-daemon/src/handlers/knowledge/kb.rs"
             "crates/missiond-daemon/src/handlers/knowledge/memory.rs"
             "crates/missiond-daemon/src/handlers/knowledge/insight.rs"
             "crates/missiond-daemon/src/handlers/knowledge/intent.rs"
             "crates/missiond-mcp/src/tools/knowledge/kb.rs"
             "crates/missiond-mcp/src/tools/knowledge/memory.rs"
             "crates/missiond-mcp/src/tools/knowledge/insight.rs"
             "crates/missiond-mcp/src/tools/knowledge/intent.rs"]
      :note "Designed V3 destination for the legacy memory/KB public tools. Code exists, but the Rust handlers are not yet physically split or runtime-projected from V3 Lisp, so this surface intentionally remains designed rather than code-aligned.")

    (surface project-registry
      :status "designed"
      :implements [project-registry project-root-resolution]
      :code ["crates/missiond-daemon/src/handlers/knowledge/project.rs"
             "crates/missiond-mcp/src/tools/knowledge/project.rs"]
      :note "Designed V3 destination for project registry and root-resolution behavior inherited from V2. Later runtime projection should make project-root policy read the V3 contract instead of ad hoc handler defaults.")

    (surface conversation-ingestion
      :status "designed"
      :implements [conversation-ingestion timeline retrospective embedding-ops]
      :code ["crates/missiond-mcp/src/tools/comm/conversation.rs"
             "crates/missiond-mcp/src/tools/comm/timeline.rs"]
      :note "Designed V3 destination for conversation/session/timeline/retrospective/embedding public tools. The current code remains legacy-shaped and must be brought under Lisp artifact/event projection before graduation.")

    (surface router-policy
      :status "designed"
      :implements [router-policy-dry-run router-backend-readiness router-dispatch-descriptor]
      :code ["crates/missiond-mcp/src/tools/comm/router_chat.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan/router_policy_dry_run.rs"
             "scripts/check-router-policy.mjs"
             "scripts/check-router-backend-registry.mjs"
             "scripts/check-router-dispatch-descriptor.mjs"]
      :note "Designed V3 destination for the V2 router-policy dry-run chain. plan/router_policy_dry_run.rs is already physically split as mission_plan's advisory runtime adapter for router-policy, backend-readiness, and dispatch-descriptor projection; this surface remains designed rather than code-aligned until the public mission_router_chat tooling is also brought under the same V3 contract. This deliberately does not claim runtime backend replacement; it only prevents router public behavior from remaining unmapped.")

    (surface incident-governance
      :status "designed"
      :implements [question incident llm-trace decision-stats auth]
      :code ["crates/missiond-mcp/src/tools/comm/question.rs"]
      :note "Designed V3 destination for question, incident, LLM trace, Gemini auth, and decision stats behavior. Later work should bind these facts into request-local events and BoardTask blocked/unblock flows.")

    (surface capability-governance
      :status "designed"
      :implements [capability-usage audit codex-ops]
      :code ["crates/missiond-mcp/src/tools/comm/capability_usage.rs"
             "crates/missiond-mcp/src/tools/comm/audit.rs"
             "crates/missiond-mcp/src/tools/comm/codex_ops.rs"]
      :note "Designed V3 destination for capability usage, audit, and Codex ops surfaces. The V2 evidence philosophy is preserved, but the handlers are not yet Lisp-isomorphic.")

    (surface compute-primitives
      :status "designed"
      :implements [pty task job flow-run process cc forge worker-control]
      :code ["crates/missiond-daemon/src/handlers/compute"
             "crates/missiond-mcp/src/tools/compute"]
      :note "Designed V3 destination for low-level worker runtime primitives: PTY, task, job, flow_run, process, CC tasks, forge, pause/control, and model process tools. Higher-level MissionD work should enter through mission_request/mission_plan/mission_board; this surface exists so legacy compute tools have a visible convergence target.")

    (surface skill-runtime
      :status "designed"
      :implements [skill-query skill-context skill-mutate skill-exec]
      :code ["crates/missiond-daemon/src/handlers/knowledge/skill.rs"
             "crates/missiond-mcp/src/tools/knowledge/skill.rs"]
      :note "Designed V3 destination for skill registry and skill execution behavior. It is mapped but not yet physically aligned to V3 pillar/function boundaries.")

    (surface cascade-governance
      :status "designed"
      :implements [universe-graph cascade-plan cascade-trigger cascade-lint]
      :code ["crates/missiond-daemon/src/handlers/knowledge/cascade.rs"
             "crates/missiond-mcp/src/tools/knowledge/cascade.rs"]
      :note "Designed V3 destination for universe graph and cascade tools. This keeps cascade/control-tree behavior explicit while later waves decide the physical split.")

    (surface sysinfra-control
      :status "designed"
      :implements [sysinfra permission power daemon-update global-instruction]
      :code ["crates/missiond-mcp/src/tools/sysinfra"
             "crates/missiond-mcp/src/tools/compute/slot.rs"
             "crates/missiond-mcp/src/tools/compute/worker.rs"]
      :note "Designed V3 destination for sysinfra MCP behavior not covered by ops-infra scripts: permissions, power, system logs/config, daemon update, global instruction, pause, and control. It remains designed until policy and runtime projection are Lisp-owned.")

    (surface ops-infra
      :status "code-aligned"
      :implements [ops-infra]
      :code ["scripts/deploy-daemon.sh"
             "scripts/cargo-fmt-touched.sh"
             "scripts/check-v3-ops-infra-isomorphism.mjs"]
      :note "deploy-daemon.sh is the canonical local redeploy path: it builds missiond-daemon, backs up the installed binary, codesigns a same-directory temp binary, atomically installs it, kickstarts launchd, waits for the IPC socket owner, then runs a bounded mission-mcp initialize smoke that supports timeout/gtimeout/perl alarm fallbacks and retries after socket readiness before rolling back on real smoke failure. cargo-fmt-touched.sh is the scoped Rust formatting path: it derives staged/unstaged/branch diff files through git diff --diff-filter=ACMR, filters existing .rs paths without failing on an empty set, skips only explicit missiond-rustfmt-exempt legacy-large-file facades during physical V3 split, and invokes rustfmt only on the remaining touched files with skip_children=true so wave-local formatting cannot churn untouched Rust modules or whole historical facades."))

  (compression-contract
    :v1 "Organized by .missiond/v1/manifest.lisp; root files remain compatibility paths."
    :v2 "Kept as historical source index, implementation status, and wave evidence."
    :v3 "Small executable contracts only: request, artifact, state-machine, policy, pillar-flow-map, implementation map."
    :checks ["node scripts/check-lisp-blueprint-compression.mjs"
             "node scripts/check-architecture-lisp.mjs --no-structure .missiond/v3/missiond-blueprint.lisp"
             "node scripts/check-v3-pillar-flow-schema.mjs"
             "node scripts/check-v3-v2-coverage.mjs"
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
             "node scripts/check-v3-source-hygiene-isomorphism.mjs"
             "node scripts/check-v3-context-pack-isomorphism.mjs"
             "node scripts/check-v3-workstation-config-isomorphism.mjs"
             "node scripts/check-v3-workstation-dispatch-isomorphism.mjs"
             "node scripts/check-v3-board-isomorphism.mjs"
             "node scripts/check-v3-ops-infra-isomorphism.mjs"
             "node scripts/check-v3-request-flow-smoke.mjs"
             "node scripts/check-v3-code-isomorphism-complete.mjs"]
    :rule "New runtime work should cite v3 first, then v2 source-index for historical evidence."))
