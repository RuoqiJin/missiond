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
      :rule "Real dispatch through mission_request execute_plan is slow + side-effecting (it creates a delegated BoardTask and may auto-provision a worker slot via mission_task_delegate). It MUST stay behind a separate, deliberately named opt-in flag (preferred name --execute-real-dispatch on scripts/check-v3-request-flow-smoke.mjs) and MUST NOT appear in default --live-ipc, --execute-dry-run, or check-v3-code-isomorphism-complete. The opt-in audit MUST pass execute=true, dry_run=false (or omit dry_run), execute_mode=internal, dispatch_strategy=agent-team, target=mission_task_delegate, cwd=<repo>, and a smoke objective that explicitly tells the delegated worker to do no file edits and no commits (read-only smoke; classify_task_kind→ReadOnly with empty owned_files so the brief instructs commit_status=not-required). The substrate (run_workstation_dispatch_with_contract_and_trace) MUST take the `WorkstationDispatchOutcome::Dispatched` branch and the response MUST surface: pipeline_result.status=executing (the plan FSM transitions to Executing on a successful Dispatched outcome — see plan.rs::build_workstation_dispatch_response), execute_mode=internal, runner_status=workstation_dispatch_v0, workstation_dispatch_status=dispatched (the substrate-level dispatch invariant emitted by outcome_to_response_fields), target_tool=mission_task_delegate, dispatch_strategy=agent-team, task_brief_preview present (non-empty), inner_result present and non-null, and a stable delegated BoardTask UUID at pipeline_result.delegated_board_task_id (projected by workstation_dispatch.rs::extract_inner_board_task_id from the inner mission_task_delegate response, which currently embeds the full BoardTask row at inner_result.task_id because compute/task_delegate.rs::handle shadows the variable name). The smoke MUST NOT wait synchronously for the delegated worker to finish; if a wait/observe mode is offered it MUST be a SECOND, separately gated, bounded option (not the default of --execute-real-dispatch). Filesystem cleanup is request-local only: --cleanup may remove .missiond/requests/<request_id>/ but MUST NOT delete the delegated BoardTask row, audit rows, or any worker-side artifacts. The checker MUST report delegated_board_task_id and the observed BoardTask status so the parent / Autopilot can observe or close the BoardTask."
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
      :rust-projection-source "crates/missiond-daemon/src/handlers/knowledge/workstation_dispatch.rs::extract_inner_board_task_id"
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

  (multi-agent-context-pack
    :desc "Two-stage parallel investigation and shard implementation as a Lisp-owned append-only context bus."
    :schema "missiond.context-pack.v1"
    :write-model "multi-agent append-only"
    :entry-heads [claim observation anchor shard-proposal conflict integration-plan]
    :mutation-owner "append helper / writer-specific entry only; no worker rewrites prior entries"
    :merge-owner "orchestrator or context-integrator appends a single integration-plan after reading proposals"
    :flow [parallel-claims parallel-observations shard-proposals conflict-notes integration-plan compile-shards dispatch-code-workers verify-and-finalize]
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
    :invariants
      ["code and research dynamic slots MUST NOT hardcode --model sonnet"
       "model=\"default\" and model_profile=coding-default-opus-4-7 both mean no CLI --model override"
       "caller-supplied model wins over model_profile, but must be a single shell token"
       "task_delegate must pass model/model_profile through to compute_slot and must not reuse an idle slot with a conflicting model override"
       "Project-bound workstation spawn MUST sync MissionD Claude hooks into <project>/.claude/settings.local.json before PTY start and MUST inject MISSION_IPC_ENDPOINT into the slot env; this preserves global ~/.claude/settings.json while making SessionStart UUID capture and UserPromptSubmit context prefetch local, idempotent, and project-scoped"
       "Autopilot pty.send budget MUST project from BoardTask.timeout_secs (default 1800s, clamped 60..7200) — never a fixed 600_000ms — so a delegated long-running task gets the timeout the delegator already declared"
       "Smart watchdog idle-recovery threshold MUST equal the projected pty.send budget plus a small grace (default 120s); only the no-PTY-session branch may reclaim sooner so a missing process can never wedge the slot"]
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
      :restart-recovery
        "Restart recovery MUST clear stale slot-dyn-* BoardTask assignee pins when the runtime slot is absent and the dynamic_slots row is not active, using BoardStore::clear_board_task_assignee before normal no-assignee routing resumes."
      :rationale
        "Wave33 evidence: a delegated BoardTask was sent twice — once via spawner.initial_prompt fire-and-forget, then again via Autopilot pty.send — and the slot's TextOutputEvent::Complete arrived without Autopilot transitioning the BoardTask to done. Single ownership of prompt+close eliminates the orphaned-task class entirely."))

  (implementation-map
    (surface mission_request
      :status "code-aligned"
      :role "single user-facing request entry"
      :code ["crates/missiond-daemon/src/handlers/knowledge/request.rs"
             "crates/missiond-mcp/src/tools/knowledge/request.rs"]
      :note "v0 request-local projections: writes request.lisp + initial lifecycle event, runs unified_entry, then projects compiled_sexp / compiled_sexp_preview into .missiond/requests/<request_id>/{intent-alignment,plan}.lisp via atomic_write_artifact and surfaces a projection status (written|skipped_*|write_failed); status action exposes artifact paths + existence booleans; review_packet (state, artifact_kind, artifact_path, artifact_exists, artifact_preview, prompt, allowed_responses, next_action, execute_allowed) is derived from request-local artifact existence + latest projection + latest review event per the unified-entry/review-packet contract — UTF-8-safe via missiond_core::util::safe_byte_truncate; respond action accepts approve_intent/reject_intent/ask_question/approve_plan/reject_plan/execute_plan, resolves directive/plan refs from explicit args, request-local intent-alignment.lisp/plan.lisp parses, or prior request-local review events; approve_intent can create a hidden BoardTask anchor before s4 plan-authoring so callers do not need to know internal board ids; approve_plan can materialize request-local plan.lisp into a persisted draft Plan row, reusing plan.lisp's BoardTask anchor when present and creating a hidden anchor only if needed, then amends request-local plan.lisp with :plan_id + :version + :board_task_id before delegating to mission_plan approve; records a request-local review event under events/<seq>.event.lisp via the same atomic_write_artifact + monotonically-increasing local sequence; delegates approve/execute decisions to mission_directive / mission_plan / unified_entry without bypassing their gates, and returns blocked responses (with next_action) when refs are missing or execute=true was not passed; approve_intent is the unified-entry bridge for the human yes step: after directive approval succeeds it immediately calls unified_entry s4 plan-authoring and projects request-local plan.lisp so the next packet asks for plan review rather than requiring a separate advance call; approve_plan moves the packet to awaiting_execution so the next legal response is execute_plan; still no DB schema migration, no auto-approval, no direct workstation dispatch. Compat-writer switch: a single compat_write_requested boolean is derived from compat_write_file (V3 name) OR legacy write_file (alias) on the caller args; both keys are then stripped from the args forwarded to mission_directive / mission_plan, and write_file=true is re-injected only when compat_write_requested is true. Default mission_request flow therefore writes ONLY request-local artifacts under .missiond/requests/<request_id>/; the compat paths .missiond/alignment/<topic>/intent-alignment.lisp and .missiond/plans/<plan_id>/PLAN.lisp are opt-in legacy projections per the (compat-writer-policy ...) contract.")

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
             "crates/missiond-daemon/src/handlers/knowledge/plan_dag.rs"
             "crates/missiond-mcp/src/tools/knowledge/plan.rs"]
      :note "compiler_mode=dry_run now renders plan-draft as an executable Lisp scaffold with :target, :objective, and :nodes; execute can derive target_source=plan_hint from plan.sexp_text instead of caller escape parameters. DAG execution parses node-local Lisp hints (:target, :objective, :timeout-ms, :target-project, :requested-cwd, :acceptance-commands, :workstation-dispatch) and forwards them into the same internal dispatch path. unified_entry owns only routing/argument forwarding: s4 compile forwards target/objective/project hints into mission_plan, and s6 execute forwards approved_plan_id + execute=true knobs without inventing a second plan schema.")

    (surface mission_workflow
      :status "code-aligned"
      :implements [workflow workflow-distiller]
      :code ["crates/missiond-daemon/src/handlers/knowledge/workflow.rs"
             "crates/missiond-mcp/src/tools/knowledge/workflow.rs"]
      :note "distill dry_run emits workflow-draft Lisp; sonnet distiller requires JSON workflow_sexp + object match_rules and validates balanced workflow_sexp before persisting a Workflow row. distill persist+write_file writes an enriched V3 workflow artifact with :workflow_id, :source_plans, :match_rules, :steps, :status, and :body workflow_sexp under ArtifactKind::Workflow at .missiond/workflows/<topic>.lisp, preserving partial-on-file-failure semantics. compile_methodology reads methodology Lisp from .missiond/workflows/<name>.lisp or workflow_path, then deterministic mode validates Lisp and emits executable YAML through the same workflow surface; its persist+write_file path now also projects the methodology compile through render_workflow_artifact_sexp so .missiond/workflows/<topic>.lisp is the same enriched V3 workflow artifact shape — :workflow_id stamped with the generated methodology flow_id, :source_plans [], :match_rules carrying source_kind=methodology / compiler / compiler_version / source_hash / flow_id / source_path / generated_at, :steps extracted from the methodology body, :status compiled (or compiled_review_required when no steps), and :body containing the methodology Lisp body — instead of canonicalizing the raw methodology source. The methodology branch still has no Workflow DB row (no workflow_id UUID, no schema migration); only the file projection is upgraded so reviewers see the same Lisp truth shape as distill. run_methodology dispatches that compiled YAML. Review gates remain receipt-only, and auto_sonnet_policy={off|safe_after_rules|dry_run} is a closed enum.")

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
             "scripts/check-verification-receipt.mjs"
             "scripts/verify-task-runner-batch.mjs"]
      :note "Task-scoped lifecycle events are first-class one-event files: the primary task-scoped path is .missiond/tasks/<wave>/events/<seq>.event.lisp (one lifecycle-event form per file, schema=missiond.task-lifecycle-event.v1, validated by check-task-lifecycle-events as standalone task-scoped event files), and task-runner-append-event allocates the next numeric file under a directory lock, validates the candidate bytes, and atomically creates them via fs.openSync(file, 'wx') when --events-dir is supplied. The legacy task-scoped task-lifecycle-events.lisp ledger is now a compatibility projection/input only: existing --ledger callers keep working unchanged, and task-runner-wave-state reads conventional task-scoped event files when present and falls back to the legacy ledger for historical waves, deduping by event id when both inputs exist. task-runner-append-event can ALSO project each append into request-local one-event files at .missiond/requests/<request_id>/events/<seq>.event.lisp when request-id/request-events-dir are supplied; task-runner-dispatch and task-runner-submit-dispatch pass both task-scoped events-dir and request-local lifecycle projection args through their real dispatch-event paths, via task-runner-next-action, instead of leaving them as append-helper-only knobs. task-runner-finalize-report can project the same finalized lineage into the V3 request-local final-report artifact at .missiond/requests/<request_id>/reports/final.lisp when request-id/request-reports-dir are supplied; the legacy report-contract remains the compatibility report. check-verification-receipt can project a single receipt into the V3 request-local verification-receipt artifact at .missiond/requests/<request_id>/receipts/<receipt_id>.lisp when request-id/request-receipts-dir are supplied through renderRequestVerificationReceipt + validateRequestVerificationReceiptSource + writeRequestVerificationReceiptFile; the writer rejects malformed request_id, malformed receipt_id, absolute or .. paths inside the resolved target, and invalid receipt objects (validateReceiptObject), validates the rendered Lisp bytes through the existing structural checker before atomic create-only rename, and refuses to overwrite an unrelated receipt file unless mode=replace and the on-disk bytes already equal the candidate; legacy task-scoped (verification-receipt-set ...) Lisp files remain compatibility inputs for readVerificationReceiptFile and verify-task-runner-batch --receipts. task-runner-append-event is the only cooperative mutation helper for task-lifecycle-event-log and task-scoped event files: it uses a sibling lock, rereads under lock, validates the candidate ledger or standalone event bytes, then atomically renames/creates outputs. task-runner-next-action prioritizes finalize_report before dispatch_task and emits dispatch events through appendLifecycleEvent. task-runner-parent-hotfix is read-only by default and projects parent patches through task-runner-finalize-report, preserving worker commit as :agent_commit_hash while :commit_hash/:final_commit_hash/:verified_commit_hash move to the final commit, and can also write the V3 request-local final-report projection. parent-hotfix finalization is a sparse Lisp projection over the worker report: task-runner-finalize-report parses the worker report's keyword/value pairs and re-emits them as-is, patching only the lineage fields (:status :commit_hash :agent_commit_hash :final_commit_hash :verified_commit_hash :parent_patches plus the unioned :files_changed) while preserving optional report-contract fields the worker already wrote (:notes :verification_tier :time_sinks :major_decisions :unexpected_work :blockers :trace_refs router-recommendation / router-readiness / router-dispatch-descriptor / verification-receipts and any additive optional fields). :acceptance_results is preserved by default; --acceptance-command appends new entries rather than replacing the worker block unless an explicit replacement opt is supplied. project-task-lifecycle-ledger backfills shared-memory/session-trace compatibility facts from lifecycle events. verify-task-runner-batch imports lifecycle validation, append-event, and parent-hotfix projections so batch smoke covers the task-scoped event files, the legacy ledger compatibility, the final-report, and the receipt path.")

    (surface context-pack
      :status "code-aligned"
      :implements [multi-agent-context-pack]
      :code ["scripts/check-context-pack.mjs"
             "scripts/context-pack-append.mjs"
             "scripts/context-pack-compile-shards.mjs"
             "scripts/check-v3-context-pack-isomorphism.mjs"]
      :note "Context-pack is the V3 high-density planning surface for two-stage parallel work: context investigators append claim/observation/anchor/shard-proposal/conflict entries to .missiond/tasks/<wave>/context-pack.lisp without code edits, then an orchestrator/integrator appends integration-plan with accepted-shards and dispatch-groups. Mapped dispatch groups use (group :id <id> :shards [...]) so scripts/context-pack-compile-shards.mjs can project the Lisp plan into dispatchable_groups for code workers; legacy bare group ids remain names_only for older packs. The structure deliberately mirrors the proven shared-memory append-only pattern but raises the semantics from lifecycle notes to implementable shard planning. scripts/context-pack-append.mjs is the cooperative mutation path: it creates a missing pack when wave/purpose are supplied, takes a sibling lock, allocates the next :seq, injects :at, validates candidate bytes, and atomically renames, including --dispatch-group-shards for mapped integration plans. scripts/check-context-pack.mjs validates missiond.context-pack.v1 headers, unique ids, strictly increasing seq, ISO timestamps, repo-relative paths, shard-proposal owner/write-scope/must-not-touch/acceptance, integration-plan accepted-shard references, mapped dispatch coverage, and accepted shard write-scope non-overlap. Code workers consume the finalized integration-plan and avoid re-deriving architecture; context investigators may run concurrently because they never rewrite prior entries.")

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
      :note "mission_compute_slot and mission_task_delegate accept model/model_profile; coder/researcher default to Claude Code Default(Opus 4.7/1M) by omitting --model. compute_slot objective is metadata only; direct warmup requires explicit initial_prompt, and delegated task_delegate auto-provision still carries suppress_initial_prompt=true. spawn_tracked_slot now syncs MissionD Claude hooks project-locally via slot_env::sync_slot_hooks_to_local_settings, preserving permissions and existing hooks while adding SessionStart session-register + UserPromptSubmit context-prefetch before PTY start; build_slot_tracking_env injects MISSION_IPC_ENDPOINT so hooks reconnect to the active daemon instead of relying on stale global defaults. Autopilot pty.send budget and smart-watchdog idle-recovery threshold are now projections of BoardTask.timeout_secs (default 1800s, clamp 60..7200, watchdog grace 120s); the no-PTY-session branch retains a 120s probe window for missing slot processes — see derive_pty_timeout_secs / idle_watchdog_threshold_secs in autopilot.rs. Autopilot prompt assembly projects the V3 prompt-tool-contract via build_base_prompt (objective dedupe) and append_board_task_id_suffix (conditional board self-close); the prompt no longer hardcodes mission_board_update / mission_board_note_add as unconditional must-calls. The V3 execution-ownership rule for delegated BoardTasks projects to: compute_slot::effective_initial_prompt + explicit initial_prompt + suppress_initial_prompt arg (delegated path starts the slot idle), task_delegate::auto_provision_slot create_args carrying suppress_initial_prompt=true, and autopilot dispatch_board_tasks holding slot_dispatch.try_acquire_guard across state.pty.send with decide_close_action preserving Done self-close and Blocked question states. Restart recovery clears stale slot-dyn-* BoardTask assignee pins via BoardStore::clear_board_task_assignee before normal no-assignee routing resumes."))

  (compression-contract
    :v1 "Organized by .missiond/v1/manifest.lisp; root files remain compatibility paths."
    :v2 "Kept as historical source index, implementation status, and wave evidence."
    :v3 "Small executable contracts only: request, artifact, state-machine, policy, implementation map."
    :checks ["node scripts/check-lisp-blueprint-compression.mjs"
             "node scripts/check-architecture-lisp.mjs --no-structure .missiond/v3/missiond-blueprint.lisp"
             "node scripts/check-v3-request-lisp-isomorphism.mjs"
             "node scripts/check-v3-intent-alignment-isomorphism.mjs"
             "node scripts/check-v3-plan-execution-isomorphism.mjs"
             "node scripts/check-v3-workflow-isomorphism.mjs"
             "node scripts/check-v3-task-lifecycle-isomorphism.mjs"
             "node scripts/check-v3-context-pack-isomorphism.mjs"
             "node scripts/check-v3-workstation-config-isomorphism.mjs"
             "node scripts/check-v3-request-flow-smoke.mjs"
             "node scripts/check-v3-code-isomorphism-complete.mjs"]
    :rule "New runtime work should cite v3 first, then v2 source-index for historical evidence."))
