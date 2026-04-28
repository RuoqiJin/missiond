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
      :ssot true
      :writer alignment-author
      :required [:request_id :objective :scope :assumptions :non_goals
                 :acceptance :risk :approval]
      :review-gate intent-review-gate)

    (artifact plan
      :schema "missiond.plan.v1"
      :path ".missiond/requests/<request_id>/plan.lisp"
      :compat-path ".missiond/plans/<topic>/PLAN.lisp"
      :ssot true
      :writer plan-author
      :required [:request_id :intent :execution :nodes :gates :approval]
      :dry-run-scaffold
        (:required-hints [:target :objective :nodes]
         :default-target mission_task_delegate
         :rule "compiler_mode=dry_run must still emit executable routing hints in Lisp; plan-runner may derive target/objective from plan.lisp without caller args"
         :non-goal "dry_run does not bypass intent/plan review and does not dispatch before execute_plan")
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
         :request-local-materialized "approve_plan may materialize a persisted plan_id by inserting a draft Plan row from request-local plan.lisp, reusing its BoardTask anchor when present and creating a hidden request-local anchor only when needed"
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
      :rule "The MCP input_schema is a projection of this Lisp review-response contract, not a permissive hidden bag; fields used for plan routing such as :target, :objective, :requested_cwd, :flow_id, :dispatch_strategy, :parallelism, :target_project, :cwd, :project, :execute_mode, :scheduler_mode, and :dry_run must be visible as explicit tool properties even when additionalProperties remains true for compatibility."
      :implementation "crates/missiond-mcp/src/tools/knowledge/request.rs builds properties structurally to avoid serde_json::json! recursion limits as the Lisp contract grows."))

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
      :rationale
        "Wave33 evidence: a delegated BoardTask was sent twice — once via spawner.initial_prompt fire-and-forget, then again via Autopilot pty.send — and the slot's TextOutputEvent::Complete arrived without Autopilot transitioning the BoardTask to done. Single ownership of prompt+close eliminates the orphaned-task class entirely."))

  (implementation-map
    (surface mission_request
      :status "code-aligned-partial"
      :role "single user-facing request entry"
      :code ["crates/missiond-daemon/src/handlers/knowledge/request.rs"
             "crates/missiond-mcp/src/tools/knowledge/request.rs"]
      :note "v0 request-local projections: writes request.lisp + initial lifecycle event, runs unified_entry, then projects compiled_sexp / compiled_sexp_preview into .missiond/requests/<request_id>/{intent-alignment,plan}.lisp via atomic_write_artifact and surfaces a projection status (written|skipped_*|write_failed); status action exposes artifact paths + existence booleans; review_packet (state, artifact_kind, artifact_path, artifact_exists, artifact_preview, prompt, allowed_responses, next_action, execute_allowed) is derived from request-local artifact existence + latest projection + latest review event per the unified-entry/review-packet contract — UTF-8-safe via missiond_core::util::safe_byte_truncate; respond action accepts approve_intent/reject_intent/ask_question/approve_plan/reject_plan/execute_plan, resolves directive/plan refs from explicit args, request-local intent-alignment.lisp/plan.lisp parses, or prior request-local review events; approve_intent can create a hidden BoardTask anchor before s4 plan-authoring so callers do not need to know internal board ids; approve_plan can materialize request-local plan.lisp into a persisted draft Plan row, reusing plan.lisp's BoardTask anchor when present and creating a hidden anchor only if needed, before delegating to mission_plan approve; records a request-local review event under events/<seq>.event.lisp via the same atomic_write_artifact + monotonically-increasing local sequence; delegates approve/execute decisions to mission_directive / mission_plan / unified_entry without bypassing their gates, and returns blocked responses (with next_action) when refs are missing or execute=true was not passed; approve_intent is the unified-entry bridge for the human yes step: after directive approval succeeds it immediately calls unified_entry s4 plan-authoring and projects request-local plan.lisp so the next packet asks for plan review rather than requiring a separate advance call; approve_plan moves the packet to awaiting_execution so the next legal response is execute_plan; still no DB schema migration, no auto-approval, no direct workstation dispatch")

    (surface mission_directive
      :status "compat"
      :implements [intent-alignment alignment-review-gate]
      :code ["crates/missiond-daemon/src/handlers/knowledge/directive.rs"
             "crates/missiond-mcp/src/tools/knowledge/directive.rs"])

    (surface mission_plan
      :status "compat"
      :implements [plan plan-review-gate plan-runner evidence-collector]
      :code ["crates/missiond-daemon/src/handlers/knowledge/plan.rs"
             "crates/missiond-daemon/src/handlers/knowledge/plan_dag.rs"
             "crates/missiond-mcp/src/tools/knowledge/plan.rs"]
      :note "compiler_mode=dry_run now renders plan-draft as an executable Lisp scaffold with :target, :objective, and :nodes; execute can derive target_source=plan_hint from plan.sexp_text instead of caller escape parameters.")

    (surface mission_workflow
      :status "compat"
      :implements [workflow workflow-distiller]
      :code ["crates/missiond-daemon/src/handlers/knowledge/workflow.rs"
             "crates/missiond-mcp/src/tools/knowledge/workflow.rs"])

    (surface task-runner-cli
      :status "compat"
      :implements [execution-lifecycle verification-receipt final-report]
      :code ["scripts/task-runner-next-action.mjs"
             "scripts/task-runner-dispatch.mjs"
             "scripts/task-runner-submit-dispatch.mjs"
             "scripts/finalize-task-report.mjs"
             "scripts/project-task-lifecycle-ledger.mjs"])

    (surface workstation-config
      :status "code-aligned-partial"
      :implements [workstation-config]
      :code ["crates/missiond-daemon/src/handlers/compute/compute_slot.rs"
             "crates/missiond-daemon/src/handlers/compute/task_delegate.rs"
             "crates/missiond-daemon/src/context/slot_env.rs"
             "crates/missiond-daemon/src/slot_orchestrator/spawner.rs"
             "crates/missiond-daemon/src/engine/intent_engine/autopilot.rs"
             "crates/missiond-mcp/src/tools/compute/compute_slot.rs"
             "crates/missiond-mcp/src/tools/compute/task_delegate.rs"]
      :note "mission_compute_slot and mission_task_delegate accept model/model_profile; coder/researcher default to Claude Code Default(Opus 4.7/1M) by omitting --model. compute_slot objective is metadata only; direct warmup requires explicit initial_prompt, and delegated task_delegate auto-provision still carries suppress_initial_prompt=true. spawn_tracked_slot now syncs MissionD Claude hooks project-locally via slot_env::sync_slot_hooks_to_local_settings, preserving permissions and existing hooks while adding SessionStart session-register + UserPromptSubmit context-prefetch before PTY start; build_slot_tracking_env injects MISSION_IPC_ENDPOINT so hooks reconnect to the active daemon instead of relying on stale global defaults. Autopilot pty.send budget and smart-watchdog idle-recovery threshold are now projections of BoardTask.timeout_secs (default 1800s, clamp 60..7200, watchdog grace 120s); the no-PTY-session branch retains a 120s probe window for missing slot processes — see derive_pty_timeout_secs / idle_watchdog_threshold_secs in autopilot.rs. Autopilot prompt assembly projects the V3 prompt-tool-contract via build_base_prompt (objective dedupe) and append_board_task_id_suffix (conditional board self-close); the prompt no longer hardcodes mission_board_update / mission_board_note_add as unconditional must-calls. The V3 execution-ownership rule for delegated BoardTasks projects to: compute_slot::effective_initial_prompt + explicit initial_prompt + suppress_initial_prompt arg (delegated path starts the slot idle), task_delegate::auto_provision_slot create_args carrying suppress_initial_prompt=true, and autopilot dispatch_board_tasks holding slot_dispatch.try_acquire_guard across state.pty.send with decide_close_action preserving Done self-close and Blocked question states."))

  (compression-contract
    :v1 "Organized by .missiond/v1/manifest.lisp; root files remain compatibility paths."
    :v2 "Kept as historical source index, implementation status, and wave evidence."
    :v3 "Small executable contracts only: request, artifact, state-machine, policy, implementation map."
    :rule "New runtime work should cite v3 first, then v2 source-index for historical evidence."))
