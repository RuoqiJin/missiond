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
      :required [:receipt_id :valid_for_files :commit_hash :tier :exit_code :commands])

    (artifact self-evolution-proposal
      :schema "missiond.self-evolution-proposal.v1"
      :path ".missiond/v3/runtime/self-evolution/<timestamp>-<finding_id>.proposal.lisp"
      :ssot false
      :writer mission_nightly_evolution
      :required [:proposal_id :finding_id :class :risk :summary :evidence_refs :affected_surfaces :recommended_change :acceptance :non_goals :created_at]
      :invariant "Proposal-only runtime evidence for MissionD self-evolution review; ignored by git, never mutates code/Lisp/checkers/KB/Board history/provider logs/worker telemetry by itself, and apply=true may create only a visible non-auto-executing review BoardTask.")

    (artifact compiled-genomes
      :schema "missiond.compiled-genomes.v1"
      :path ".missiond/v3/runtime/compiled/compiled-genomes.json"
      :ssot false
      :writer missiond-lispc.emit-genomes
      :required [:schema_version :source_hash :diagnostics :payload]
      :invariant "Runtime projection for genome organs/cells/tissues; daemon hot paths load compiled JSON or explicit fallback, never parse raw genome Lisp in process."))

  (ssot-retrieval-scope
    :schema "missiond.ssot-retrieval-scope.v1"
    :purpose "Keep SSOT review and worker context small by separating authoring truth from generated runtime evidence."
    (tier active-authoring
      :default true
      :paths [".missiond/v3/missiond-blueprint.lisp"
              ".missiond/v3/shards/*.lisp"
              ".missiond/v3/genome/*.lisp"
              ".missiond/workflows/*.lisp"
              ".missiond/frontend/board-blueprint.lisp"
              ".missiond/**/intent.lisp"
              ".missiond/**/*blueprint.lisp"]
      :rule "Default SSOT review, M6 review, nightly evolution, and worker context-pack generation read active-authoring paths first.")
    (tier warm-evidence
      :default "explicit-reference-only"
      :paths [".missiond/v3/evidence/*.lisp"
              ".missiond/frontend/evidence/*"
              ".missiond/research/*.md"]
      :rule "Warm evidence is used when an active blueprint cites it, or when a task asks for architecture history.")
    (tier cold-runtime
      :default false
      :paths [".missiond/v3/runtime/**"
              ".missiond/tasks/**"
              ".missiond/research/memory-review*/**"]
      :examples [".missiond/v3/runtime/lisp-code-sync/*.report.lisp"
                 ".missiond/v3/runtime/nightly-evolution/*.report.lisp"
                 ".missiond/v3/runtime/genome/*.json"
                 ".missiond/v3/runtime/self-evolution/*.proposal.lisp"
                 ".missiond/v3/runtime/master-control/context-packs/*.lisp"
                 ".missiond/v3/runtime/compiled/*.json"]
      :rule "Cold runtime artifacts are diagnostic/query targets, not authoring SSOT. They are excluded from broad rg/review/search unless include_runtime=true or a concrete trace/report path is requested.")
    :invariants
      ["Tools that answer 'what does the SSOT say?' MUST search active-authoring first and exclude cold-runtime by default."
       "Generated compiled JSON and runtime reports are projections/evidence; they must not be treated as editable blueprint source."
       "MissionD may query cold-runtime for trace/debug/report lookup, but that query must be explicit and visible in the context-pack."]
    :checker "node scripts/check-v3-runtime-path-hygiene.mjs")

  (blueprint-shard-index
    :schema "missiond.blueprint-shard-index.v1"
    :index ".missiond/v3/shards/index.lisp"
    :root ".missiond/v3/missiond-blueprint.lisp"
    :status compiler-active
    :rule "The root blueprint remains the compiler entrypoint, but compiler-active shards included from root are executable SSOT source units. Runtime behavior may depend on a shard only through missiond-lispc resolved projections."
    :shards [typed-compiler-runtime-projection workstation-policy-shards project-universe-shards v2-convergence-map pillar-flow-map implementation-map]
    (shard v2-convergence-map
      :path "shards/v2-convergence-map.lisp"
      :status compiler-active)
    (shard pillar-flow-map
      :path "shards/pillar-flow-map.lisp"
      :status compiler-active)
    (shard implementation-map
      :path "shards/implementation-map.lisp"
      :status compiler-active))

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
      :mcp-config "$MISSION_HOME/xjp-mcp-config.json"
      :default-cwd "/Users/jinchen/Projects")
    (slot-template researcher
      :role coder
      :description "Dynamic researcher slot (read-only analysis)"
      :default-model-profile research-default
      :mcp-config "$MISSION_HOME/xjp-mcp-config.json"
      :default-cwd "/Users/jinchen/Projects")
    (slot-template ops
      :role operator
      :description "Dynamic ops slot (ephemeral)"
      :default-model-profile daily-sonnet
      :mcp-config "$MISSION_HOME/xjp-mcp-config.json"
      :default-cwd "/Users/jinchen/Projects")
    (cwd-policy dynamic-slot
      :allowed-prefixes ["/Users/jinchen/Projects" "/Users/jinchen/Downloads" "/Users/jinchen/Documents" "/tmp"])
    (chat-completions-policy jarvis-api
      :default_slot "slot-claude-code-default"
      :header_override "X-Slot-Id"
      :rule "OpenAI-compatible /v1/chat/completions routes to the explicit X-Slot-Id header when present; otherwise it uses this V3-projected default slot. Rust must not hardcode slot-jarvis.")
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
    (capacity-policy swarm-workers
      :default_claude_workers 8
      :max_claude_workers 16
      :default_gemini_workers 2
      :max_gemini_workers 6
      :dynamic_slot_limit 20
      :delegate_rate_per_minute 24)
    (ttl-policy dynamic-slot
      :default_secs 14400
      :min_secs 300
      :max_secs 28800
      :default_extend_secs 3600
      :max_extend_secs 3600)
    (managed-node-runtime-policy host-portability
      :mcp-config-resolution host-relative
      :mcp-config-placeholders ["$MISSION_HOME"]
      :registered-project-roots-allowed true
      :db-integer-portability [ttl_seconds extend_count message_count])
    (pty-provider-unavailable-policy provider-blocked-diagnostics
      (state auth_missing
        :state blocked
        :blocked-kind auth_missing
        :keywords [credentials credential login auth authenticated])
      (state billing_or_account
        :state blocked
        :blocked-kind billing_or_account
        :keywords [billing payment subscription suspended paused account])
      (state usage_limit
        :state blocked
        :blocked-kind usage_limit
        :keywords [quota limit rate-limit usage]))
    :invariants
      ["code and research dynamic slots MUST NOT hardcode --model sonnet"
       "daemon startup SlotManager ClaudeCode task configs MUST project coder/researcher model profiles from workstation-config and omit --model for coding-default-opus-4-7"
       "daemon startup SlotManager task configs MUST be generated from workstation-config startup-slot entries, including engine/lifecycle/slot_id/role/timeout_secs/skip_permissions"
       "daemon startup MUST resolve the MissionD orchestrator root from MISSIOND_PROJECT_ROOT, MISSIOND_ORCHESTRATOR_ROOT, or the current working directory ancestor containing .missiond/v3/missiond-blueprint.lisp; runtime code MUST NOT hardcode /Users/jinchen/Projects/missiond as the orchestrator root because managed machines may install MissionD under their own user home."
       "Clean-machine daemon startup MUST create missing provider history watch directories such as ~/.claude/projects before registering filesystem watchers; absence of prior ClaudeCode/Codex/Gemini history is not a fatal condition for a managed MissionD node."
       "mission_compute_slot dynamic template role/description/mcp_config/default_cwd and allowed cwd prefixes MUST project from workstation-config slot-template + cwd-policy dynamic-slot, not a Rust-local template table; dynamic slots MUST also allow cwd under any registered active ProjectRegistry root so managed nodes installed under a different user home (for example /Users/rickyhq/Projects/missiond) can run without host-specific Lisp rewrites."
       "ClaudeCode mcp_config MUST be host-relative: workstation-config may use $MISSION_HOME/xjp-mcp-config.json, and PTY launch MUST resolve stale or missing xjp-mcp-config.json paths to the current host's MissionD home before spawning. MissionD blue-green deploy MUST create a host-local xjp-mcp-config.json with at least the missiond MCP server when it is missing, so fresh managed nodes do not fail with a literal $MISSION_HOME path. A managed node MUST NOT inherit /Users/jinchen/.xjp-mission/xjp-mcp-config.json as an executable truth. If a slot keeps a live PTY process while its public agent_info state is Exited/Error, readiness MUST classify stale_slot and spawn MUST clean the stale session before respawn instead of reporting both already-running and exited."
       "PTY recognition MUST classify provider unavailable states as blocked diagnostics, not completed turns: auth_missing covers missing credentials/login-required screens, billing_or_account covers paused/suspended/payment/subscription failures, and usage_limit covers quota/rate-limit exhaustion. Exited/Error SessionState may override stale running text, but MUST preserve these provider-unavailable blocked snapshots so Board/Terminal can show the real action required."
       "Dynamic slot database rows MUST decode ttl_seconds and extend_count portably from Postgres INTEGER or BIGINT so clean managed nodes initialized from current migrations do not panic with int4/i64 mismatches."
       "Jarvis/OpenAI-compatible chat completions default slot MUST project from workstation-config chat-completions-policy jarvis-api; X-Slot-Id remains the explicit request override and Rust MUST NOT hardcode slot-jarvis."
       "model=\"default\" and model_profile=coding-default-opus-4-7 both mean no CLI --model override"
	       "mission_compute_slot model_profile resolution MUST use workstation-config model-profile spawn-model-arg, not a Rust-local profile table"
	       "caller-supplied model wins over model_profile, but must be a single shell token"
	       "task_delegate must pass model/model_profile through to compute_slot and must not reuse an idle slot with a conflicting model override"
	       "mission_task_delegate MUST accept structured two-stage delegation metadata (task_class, pool_hint, engine_hint, context_pack_path, read_scope, write_scope, must_not_touch, acceptance) and persist it into the BoardTask description so Autopilot workers see context-pack path, explicit readable evidence, exact write scope, forbidden write paths, and acceptance commands without relying on side-channel PTY text. The generated scope_semantics line MUST state that must_not_touch forbids write/stage/commit and is not a read ban by itself; review/context-pack/research tasks MUST carry an output_contract requiring a structured artifact with Findings / Evidence / Recommendations / Verification rather than raw KB JSON or full logs."
	       "mission_task_delegate duplicate-code-worker guard MUST compare relative write_scope entries inside their resolved BoardTask.project/project_root only; cross-project sibling tasks may all write .missiond/check.sh or .missiond/evidence/current-code-mapping.md without false DUPLICATE_CODE_WORKER_BLOCKED refusals. Absolute write_scope entries remain globally comparable."
	       "mission_task_delegate MUST NOT auto-preload KB/Skill context from context_hints into worker prompts by default; current KB/skill stores are noisy and hidden prompt injection obscures task contracts. Context must come from explicit read_scope, context_pack_path, task contract, or a future explicit memory-audit workflow."
		       "Autopilot context prefetch defaults disabled until memory stores are cleaned: delegated worker prompts MUST NOT prepend KB/Skill/context-pipeline output unless an explicit memory-audit workflow opts in via MISSIOND_AUTOPILOT_CONTEXT_PREFETCH=1."
	       "mission-mcp initialize MUST NOT inject KB summary / search-path instructions by default; noisy memory context is opt-in only via MISSIOND_MCP_PRELOAD_INSTRUCTIONS=1 for explicit memory-audit sessions."
	       "Deploy/CI wait loops MUST use deploy-center provenance/events or bounded XJP MCP wait/watch tools such as xjp_build_wait and xjp_deploy_watch. Workers MUST NOT repeatedly poll GitHub Actions with raw gh api loops; GitHub API calls are diagnostic snapshots only, not the waiting mechanism."
	       "mission_swarm_run MUST honor max_gemini_workers exactly: when max_gemini_workers=0, no spawned context-pack BoardTask may use intent=research or any other routing signal that sends the task to Gemini; Claude context-pack workers use code/coder routing plus read-only completion protocol."
	       "mission_swarm_run MUST resolve project_id to a registered project_root before creating external-project BoardTasks; generated swarm metadata and default read_scope must include that project_root so Autopilot can spawn provider PTYs in the target project instead of MissionD's own cwd. For cross-project universe work, mission_swarm_run MUST accept target_project_ids/targetProjectIds, resolve every id through ProjectRegistry, render target_projects into the worker Swarm metadata and missiond.swarm-context-pack.v1 sidecar, and merge every target root into read_scope so workers can inspect all declared projects without relying on prompt prose. When target_project_ids is present and more than one context-pack worker is requested, mission_swarm_run MUST partition target project roots across workers by default, while preserving any non-target caller read_scope entries for every worker; duplicate all-target broad audits are allowed only through an explicit caller read_scope override. If a child task's read_scope/write_scope resolves to exactly one target project, the child BoardTask.project, Swarm metadata project_id/project_root, and auto-provisioned dynamic slot cwd/project_root MUST be that target project rather than the orchestrator project. Worker-facing context_pack_path MUST be an absolute MissionD workspace path (or an already absolute caller path), because external-project workers run with cwd at the target project root and relative .missiond paths would point at the wrong project. For non-dry-run dispatch, mission_swarm_run MUST materialize a missiond.swarm-context-pack.v1 sidecar at context_pack_path before publishing worker BoardTaskCreated events."
	       "mission_swarm_run MUST auto-provision per-Claude dynamic slots by default for non-dry-run Claude context-pack / implement shards and persist each created slot id as the child BoardTask assignee before publishing BoardTaskCreated. This is the productized fanout path for M6 SSOT waves; otherwise all children collapse onto the single persistent ClaudeCode slot. auto_provision_slots=false is allowed only as an explicit diagnostic override."
	       "mission_swarm_run context-pack/read-only lanes MUST render write_policy=read-only and the strict no-edit/no-stage/no-commit completion protocol even when the parent wave write_policy is lisp-first or code; write permission is granted only to lanes with a non-empty write_scope."
	       "mission_swarm_run MUST fail fast with SWARM_IMPLEMENT_WRITE_SCOPE_REQUIRED when write_policy is not read-only and the caller did not pass an explicit write_scope; the tool must never dry-run or dispatch an implementation shard with an empty write_scope because that turns disjoint ownership into prompt prose."
	       "mission_swarm_run MUST fail fast with SWARM_ACCEPTED_SHARD_REQUIRED when write_policy is not read-only and the caller did not pass accepted_shard_id/acceptedShardId; implementation workers consume already-accepted exact shards, never broad M6 objectives."
	       "mission_task_delegate MUST fail fast with EXACT_SHARD_CONTEXT_PACK_REQUIRED / EXACT_SHARD_ID_REQUIRED when a code/implementation worker declares write_scope but lacks context_pack_path or accepted_shard_id. Broad review/design goals belong to investigator lanes; implementation lanes must name the accepted shard id."
	       "Implementation worker prompts MUST explicitly forbid internal ClaudeCode TaskCreate/TaskUpdate subagent delegation; recursive decomposition belongs to MissionD master/workflow and is audited from provider durable logs as a workflow violation."
	       "mission_task_delegate and mission_swarm_run MUST accept parent_id/parentId aliases and persist them into CreateBoardTaskInput.parent_id when a master objective spawns child shards; mission_swarm_run MUST also render parent_board_task_id in the worker-facing Swarm metadata so the Board hierarchy, parent-note closeout, and master recovery loop can connect child completion evidence back to the active objective."
	       "Autopilot ensure_pty MUST override pty_slot.cwd to the BoardTask.project's registered project_root when the BoardTask carries a project label that resolves under ProjectRegistry and that root differs from slot.config.cwd; spawn_tracked_slot's project-root-spawn-cwd contract then handles Gemini/Codex hard-fail and ClaudeCode normalization. Slot reuse for cross-project dispatch MUST require slot.project_root == BoardTask project_root (already enforced for mission_task_delegate; mission_swarm_run BoardTasks rely on the spawn-side cwd override for the same effect)."
	       "Autopilot MUST unclaim a BoardTask when ensure_pty returns false after a claim, because spawn-pending / busy / transient PTY states are retryable dispatch conditions and must not wedge the task in running with no prompt delivered."
	       "Autopilot ensure_pty MUST treat `PTY session already running` from spawn_tracked_slot as a pre-provisioned dynamic-slot race: wait briefly for Idle and reuse the PTY instead of recording a spawn failure note or incrementing BoardTask retry. If the session remains non-idle, it is a retryable busy condition and the task must be unclaimed for a later tick."
	       "Autopilot/flow-engine BoardTask dispatch MUST bind conversations.task_id to the active BoardTask via a bounded retry helper at dispatch time (5 attempts at 200 ms) and MUST re-bind after the worker final settle window to cover provider JSONL/session-discovery races; completion-time durable_provider_completion_for_slot_task remains a fallback. The dispatch site is the single rebind authority: when a new BoardTask claims a slot whose conversation row carries a different earlier task_id, Autopilot MUST authoritatively overwrite conversations.task_id to the incoming task. The conservative `conversation_task_binding_update_allowed` predicate is reserved for the post-completion durable backfill path (set only when unbound or already matching); the pre-dispatch path uses a force-rebind helper that logs the displaced task id for audit. Historical attribution lives in durable messages (mission_conversation_query(taskId=...) MUST also recover provider conversations whose durable messages contain the BoardTask id), so a stale conversations.task_id pointer is unnecessary and previously caused mission_conversation_query(taskId=<new>) to return the prior task's conversation (BoardTask 31e5449c-e315-4003-ad59-c3eebd5eb837 evidence: slot-claude-code-default returned the 5599b07a conversation when queried by 738c96f5)."
       "Autopilot dispatch MUST enforce a single-running-BoardTask-per-slot invariant: before claim_board_task, scan running tasks for any other BoardTask whose claim_executor_type=pty_slot and claim_executor_id matches the incoming slot id, and unclaim each one with a durable note. A queued task with assignee=slot but no claim_executor_id is NOT considered running on the slot and MUST NOT be unclaimed by this guard. The display layer (handlers/compute/slot.rs `active_board_task_for_slot`) projects the slot's running task from this single-claim invariant, so two tasks can never appear running on the same slot for the same dispatch tick."
       "Autopilot close path MUST gate PTY-only completion (durable provider final unavailable after settle) for delegated worker BoardTasks (description carries `## Swarm metadata` or `## Dispatch metadata`). pty_only_close_blocker requires the PTY summary to contain a structured artifact marker (Findings / Evidence / Recommendations / Verification / Summary heading / acceptance evidence) before close; otherwise the BoardTask stays running so the watchdog/next tick can re-extract once the provider log lands. If the worker description declares output_contract Findings / Evidence / Recommendations / Verification, output_contract_close_blocker applies even when a durable provider summary exists, because a reused provider session may expose an older task's accepted summary before the current task's final artifact lands. Workflow-specific structured artifacts may satisfy the same contract when they carry Findings + Verification plus explicit candidate/rationale sections; memory-review-batch-runner accepts Active Memory Candidates / SSOT-Workflow Backfill Candidates / Needs Human / Discard Rationale as the Evidence/Recommendations equivalent. Repro evidence: BoardTask 31e5449c-e315-4003-ad59-c3eebd5eb837 child tasks a5ebf6c4..., 5599b07a..., b5be6eed... had Board summary notes that captured an intermediate assistant sentence while the structured artifact landed only in Claude JSONL after settle; BoardTask 7b5f3174... briefly picked a prior M10 overlay summary before the current context-pack's Findings/Evidence/Recommendations/Verification report arrived; memory review child e1ea8d06... produced valid memory-review sections but was repeatedly blocked as missing-output-contract-sections until the contract admitted workflow-specific artifact headings."
       "Autopilot durable final acceptance evidence MUST recognize provider final summaries that say gates green/pass, checks pass, checker passed, check.sh passed, acceptance commands passed, or final M10 evidence-only gate confirmation, not just legacy words like verified/passed/changed files. Repro evidence: M10 child tasks 5ecb01cd..., d699c9c7..., 66aed32d..., ae72b0ec..., and f6c5475d... produced durable ClaudeCode finals with gate/checker completion language but were incorrectly blocked as missing-acceptance-evidence."
       "mission_swarm_run callers (resident master, autopilot, ad-hoc operators) MUST pass multi-project objectives via target_project_ids/targetProjectIds/target_projects/targetProjects structurally; project lists embedded only in the objective prose are ignored by the tool because the schema does not parse natural language. The MCP schema MUST expose target_project_ids and aliases as an array property of mission_swarm_run so MCP clients can pass it without guessing naming conventions. Failure mode (BoardTask 31e5449c regression): when only project_id was supplied and the objective text named multiple registered projects in prose, target_projects collapsed to project_id only and the swarm could not fan out across the universe."
	       "Autopilot MUST treat explicit engine_hint/pool_hint as hard constraints when the V3 workstation-pool declares at least one matching worker: resolve matching workers against the full workstation-pool before task_class fallback can narrow candidates away; if that worker is busy or stopped, the task waits instead of silently spending a different provider. Fallback to a non-matching worker is allowed only when no declared worker satisfies the hint at all, and that fallback MUST record a durable reroute_reason as a BoardTask note before dispatching so the operator can see why the requested engine/pool was not used. Autopilot close-owner MUST block task close when a worker final says it could not write the requested deliverable because of plan/read-only mode."
	       "mission_task_delegate intent=research without an explicit Claude coding model/model_profile MUST prefer the workstation-pool gemini researcher slot (slot-gemini-ultra) when registered; the researcher slot-template's :default-model-profile is research-default, which binds to the gemini-ultra worker. Auto-provisioning a dynamic Claude slot for research is forbidden while a V3 gemini researcher slot exists; the BoardTask is queued unassigned and the autopilot routes it to the gemini slot once idle. Explicit model_profile=coding-default-opus-4-7 (or any Claude profile) still routes the BoardTask to Claude."
       "Project-bound workstation spawn MUST sync MissionD Claude hooks into <project>/.claude/settings.local.json before PTY start and MUST inject MISSION_IPC_ENDPOINT into the slot env; this preserves global ~/.claude/settings.json while making SessionStart UUID capture local, idempotent, and project-scoped. UserPromptSubmit context prefetch MUST be opt-in only through MISSIOND_CLAUDE_CONTEXT_PREFETCH=1 and the hook sync MUST remove missiond-context-inject-v2.sh from project settings by default while KB/history stores are noisy."
       "Autopilot pty.send budget MUST project from BoardTask.timeout_secs (default 1800s, clamped 60..7200) — never a fixed 600_000ms — so a delegated long-running task gets the timeout the delegator already declared"
       "mission_cc_swarm pty.send budget MUST project from workstation-config timeout-policy claudecode-swarm (default 600s, clamped 60..7200) — never a local 600_000ms literal"
       "mission_pty_send waitForResponse budget MUST project from workstation-config timeout-policy pty-send-blocking (default 300s, clamped 1..7200) — never a local 300_000ms literal"
       "mission_compute_slot and Claude/Gemini slot-orchestrator dynamic slot spawn wait_for_idle timeouts MUST project from workstation-config timeout-policy dynamic-slot-spawn (default 60s, clamped 10..600) — never local Some(60)/Some(120) literals"
       "context-pack-run-wave default worker fanout MUST project from workstation-config dispatch-policy context-pack-run-wave (default 4, clamped 1..8), while caller --max-parallel remains an explicit override"
       "mission_swarm_run fanout defaults/caps and dynamic slot limits MUST project from workstation-config capacity-policy swarm-workers (default Claude 8 max 16, default Gemini 2 max 6, dynamic slots 20, delegate rate 24/min) so supervised waves can scale without divergent Rust constants"
       "Dynamic slot TTL and per-request extension budget MUST project from workstation-config ttl-policy dynamic-slot (create default 14400s, clamped 300..28800; extend default/max 3600s) — direct mission_compute_slot create/extend and delegated task_delegate auto-provision must not hardcode the TTL window"
       "Smart watchdog idle-recovery threshold MUST equal the projected pty.send budget plus a small grace (default 120s); only the no-PTY-session branch may reclaim sooner so a missing process can never wedge the slot"
       "Autopilot BoardTask claim lease MUST equal the smart-watchdog idle-recovery threshold (projected pty.send budget plus grace); the legacy fixed 20-minute lease is forbidden because it lets the watchdog reclaim a slot whose claim is still legitimately ticking inside the declared timeout"
       "Autopilot summary-note source MUST prefer durable provider final evidence after wait_for_worker_final_settle_window(), per-session reconcile, and a bounded await_durable_provider_completion_for_slot_task poll. Inside durable provider evidence, provider_completion_summary_for_task MUST prefer latest_assistant_after_task_prompt for the current BoardTask before any conversation.task_id latest-after-claim fallback, because a single provider session may execute multiple sequential BoardTasks and later rebinds must not let an older final leak into the current task. Durable assistant messages that are tool invocations, survey/progress narration such as 'Checking ...', 'Surveying ...', 'Reading ...', 'Inspecting ...', 'Reviewing ...', 'Looking at ...', or 'Gathering ...', initial worker-intent narration such as 'I'll execute...' / 'I'll begin by reading...' / 'Acknowledged... I will redo...' / 'Let me start/re-verify...', intermediate investigation narration such as 'Let me ...' / 'Let me inspect/check/read/verify/confirm/corroborate/write/create/examine...' / 'Now I'll ...' / 'Now I will ...', mutation-progress narration such as 'Now committing...' / 'staging and committing...' / 'Writing the ... now', or retry/wakeup blocker narration such as 'wakeup will fire' / 'scheduled to retry' / 'wait for that retry' / 'ENOSPC' / 'no space left on device' are not valid finals. Raw res.response is forbidden in the **Autopilot 执行完成** note format string and in the synthesized mission_execution(action=complete) summary; fallback extract_worker_final_summary(res.response, full_prompt) is allowed only after durable evidence is unavailable and MUST strip bare tool-call lines such as Bash(...), ●/⎿ tool logs, echoed task contract, and `[Pasted text +N lines, paste again to expand]` collapse markers. Auth-error and quota-exhausted diagnostic notes intentionally bypass this path and keep the raw response so on-call sees the verbatim platform error"
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
	         :summary-note-source "The `**Autopilot 执行完成**` BoardTask summary note and the synthesized mission_execution(action=complete) summary MUST prefer durable provider final evidence after settle + per-session reconcile + bounded durable-final polling (claude-jsonl/codex-sqlite/gemini-chat-file), but durable assistant tool-invocation records such as `[Tool: Bash] ...`, active/progress frames, initial worker-intent narration such as 'I'll execute...' / 'I'll begin by reading...' / 'Acknowledged... I will redo...' / 'Let me start/re-verify...', intermediate investigation narration such as 'Let me ...' / 'Let me inspect/check/read/verify/confirm/corroborate/write/create/examine...' / 'Now I'll ...' / 'Now I will ...', mutation-progress narration such as 'Now committing...' / 'Writing the ... now', and retry/wakeup blocker narration such as 'wakeup will fire' / 'scheduled to retry' / 'wait for that retry' / 'ENOSPC' / 'no space left on device' are not valid finals; only fall back to extract_worker_final_summary(res.response, full_prompt) when no valid durable final exists after polling. The note body is capped via truncate_safe, and passing raw res.response into the note format string is forbidden because the Claude Code TUI screen capture includes the echoed prompt + task contract, bare Bash(...)-style tool-call lines, ●/⎿ tool log lines, and `[Pasted text +N lines, paste again to expand]` collapse markers. Auth-error and quota-exhausted diagnostic notes intentionally bypass this path and keep the raw response so on-call operators see the verbatim platform error."
	         :settle-window "After pty.send returns Complete, Autopilot MUST wait through wait_for_worker_final_settle_window() and then poll durable provider evidence for one bounded settle budget before writing the summary note, synthesizing mission_execution completion, or transitioning the BoardTask to Done; default settle is intentionally long enough for provider JSONL/SSE final text to land, and may be overridden only by MISSIOND_AUTOPILOT_FINAL_SETTLE_MS."
	         :commit-failure-blocker "If the worker final indicates a blocking commit/tool failure such as GPG pinentry cancellation, commit failed, failed to commit, or could not commit, Autopilot MUST NOT mark the BoardTask done or synthesize a successful mission_execution completion. It writes an autopilot blocker note, transitions the task to Blocked, and leaves recovery to a supervisor/worker with explicit scope."
	         :idle-durable-summary-close "If pty.send returned an active/progress frame and left the BoardTask running, a later watchdog tick MAY close only when the claimed slot is Idle AND either get_board_task_with_notes shows a claim-after durable summary note OR the provider conversation store has a claim-after task prompt plus assistant final for the same BoardTask. Provider-final closure must synthesize a BoardTask summary note and backfill conversation.task_id before Done; this closes from durable evidence plus idle diagnosis, never from PTY idle alone."
	         :blocked "If the task transitioned to Blocked (e.g. mission_question_create) during execution, Autopilot preserves the Blocked state on pty.send return and never overwrites it with done.")
      :dispatch-guard
        "The per-slot dispatch guard MUST be held across the entire state.pty.send call; the legacy release-before-send pattern allowed a second caller to dispatch to the same slot mid-flight. The guard is per-slot, so holding it does not starve callers targeting other slots."
      :concurrent-slot-dispatch
        "Autopilot dispatch_board_tasks MUST start state.pty.send work concurrently across different slots and MUST NOT wait for worker turn completion inside the dispatch tick. The legacy serial loop awaited one slot's pty.send before any other slot's send could begin, and the later JoinSet-drain variant still starved newly-idle pre-provisioned dynamic slots because the tick did not return until early workers finished. The implementation MUST hand each ready BoardTask's send + post-send tail to a detached tokio task with an OwnedSlotDispatchGuard moved in, so same-slot exclusion covers the entire send + close-owner / KB-feedback / deploy-review sequence while the Autopilot event loop can keep dispatching other idle slots on later ticks. Quota / global-pause / KB-feedback / retry semantics run inside that background tail and MUST update durable BoardTask/event state instead of relying on the dispatch tick return."
      :restart-recovery
        "Restart recovery MUST clear stale slot-dyn-* BoardTask assignee pins when the runtime slot is absent and the dynamic_slots row is not active, using BoardStore::clear_board_task_assignee before normal no-assignee routing resumes."
	    :rationale
	        "Wave33 evidence: a delegated BoardTask was sent twice — once via spawner.initial_prompt fire-and-forget, then again via Autopilot pty.send — and the slot's TextOutputEvent::Complete arrived without Autopilot transitioning the BoardTask to done. Single ownership of prompt+close eliminates the orphaned-task class entirely.")
    (claude-code-mcp-recovery
      :desc "Lisp-owned ClaudeCode MCP reconnect ritual and missing-MCP incident contract; the mounting and reconnect navigation are Lisp-pinned, never tool-registry-decided at runtime."
      :reconnect-keystrokes ["/mcp" "<enter>" "<arrow-down>*N" "<enter>" "<enter>"]
      :forbid-numeric-shortcut true
      :missing-incident-kind "claude_code_mcp_missing"
      :reconnect-failed-incident-kind "claude_code_mcp_reconnect_failed"
      :reconnect-budget-attempts 1
      :wake-resident-master true
      :surfaces ["crates/missiond-pty/src/session.rs::Session::mcp_reconnect_sequence"
                 "crates/missiond-pty/src/session.rs::PTYSession::mcp_reconnect"
                 "crates/missiond-pty/src/manager.rs::PTYManager::mcp_reconnect"
                 "crates/missiond-daemon/src/workers/local/pty_event_worker.rs::handle_mcp_tool_error"
                 "crates/missiond-daemon/src/engine/master_control.rs::spawn_incident_event_sub"]
      :rationale "Claude Code's /mcp picker numeric shortcuts have shifted between TUI versions; arrow-key navigation is the only stable substrate. When supports_mcp=true is advertised but no mission_* tool surfaces after slot ready, the worker is operating without orchestration tools and the master must be woken via a durable incident, not a silent reconnect retry loop."))

  (workstation-policy-shards
    :desc "Split workstation-config invariants into policy shards so runtime, checkers, and agents can reason over one small policy at a time."
    (policy slot-lifecycle-policy
      :owns [startup-slot dynamic-slot ttl heartbeat release stale-slot-reap]
      :core ((step s1 :logic "reserve or create a slot with projected model/cwd/ttl")
             (step s2 :logic "bind active task, conversation, and claim lease")
             (step s3 :logic "heartbeat while provider is running")
             (step s4 :logic "release or mark stale after durable completion or TTL expiry"))
      :surface workstation-config)
    (policy delegation-contract-policy
      :owns [task_delegate swarm_run accepted_shard write_scope read_scope completion_protocol]
      :core ((step s1 :logic "classify investigator versus implementer before dispatch")
             (step s2 :logic "require context_pack_path and accepted_shard_id for implementation lanes")
             (step s3 :logic "persist read/write scope and must-not-touch into BoardTask metadata")
             (step s4 :logic "reject broad objectives in code-worker lanes before provider spawn"))
      :surface workstation-config)
    (policy completion-authority-policy
      :owns [provider_final task_result_artifact board_note_projection pty_diagnostic]
      :core ((step s1 :logic "prefer durable provider final over PTY")
             (step s2 :logic "normalize final output into task-result-artifact")
             (step s3 :logic "project concise summary to Board note and mission_execution")
             (step s4 :logic "close BoardTask only after settle window and artifact validation"))
      :surface task-result-artifact)
    (policy cross-project-dispatch-policy
      :owns [project_root cwd read_scope target_project_ids external_project_worker]
      :core ((step s1 :logic "resolve every project id through ProjectRegistry")
             (step s2 :logic "materialize absolute context_pack_path and project roots")
             (step s3 :logic "spawn provider with target project cwd or explicit readable scope")
             (step s4 :logic "record reroute or block reason when project evidence is missing"))
      :surface project-registry)
    (policy context-prefetch-policy
      :owns [kb_prefetch skill_prefetch explicit_context memory_audit]
      :core ((step s1 :logic "default to explicit context_pack/read_scope only")
             (step s2 :logic "allow KB/skill prefetch only in explicit memory-audit or operator-approved sessions")
             (step s3 :logic "redact noisy or unreviewed memory before worker prompt projection")
             (step s4 :logic "record prefetch source and reason as diagnostic evidence"))
      :surface memory-kb)
    (policy mcp-recovery-policy
      :owns [mcp_ready reconnect_ui navigation_hint provider_tool_availability]
      :core ((step s1 :logic "detect missing MCP readiness from slot/provider diagnostics")
             (step s2 :logic "surface human-like reconnect navigation hints without numeric shortcut assumptions")
             (step s3 :logic "route repeated failure to BoardTask and not hidden prompt retry")
             (step s4 :logic "only resume worker dispatch once readiness is verified"))
      :surface capability-governance)
    :checker "node scripts/check-v3-workstation-dispatch-isomorphism.mjs")

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
    (worker claude-code-deploy-ops
      :engine claude-code
      :role deploy-ops
      :slot-id "slot-claude-code-deploy-ops"
      :task-type claude_code_deploy_ops
      :model-profile coding-default-opus-4-7
      :model nil
      :task-classes [deploy-ops deployment ops incident-response]
      :capabilities [deploy-read deploy-observe deploy-center-query rollback-plan mcp]
      :max-concurrency 1
      :timeout-secs 2400
      :default-use deployment-operations
      :accepts-boardtask true
      :write-allowed false)
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
       "Deployment/Ops work MUST route to the explicit claude-code-deploy-ops lane when context_intent=deploy-ops or task_class=deploy-ops is present; generic claude-code-default is not the default deployment lane. Deploy-ops is observation/planning first: it may query deploy-center, provenance, skills, and logs, but production mutation requires deploy-center policy or explicit Board approval. CI/build waiting uses deploy-center events/provenance or bounded XJP MCP wait/watch tools; raw gh api polling loops are forbidden."
       "Claude fast-patch may use Sonnet only for narrow atomic tasks whose context-pack already identifies exact files/regions; it is not a default coding lane."
       "Autopilot must resolve workstation-pool dispatch by task class before hints: `pool_hint` may select a concrete worker across the full pool, but `engine_hint` alone only filters/ranks the current task-class candidates and MUST NOT widen a complex `task_class=code` shard into the `claude-code-fast-patch` Sonnet lane."
       "Gemini Ultra Pro is the high-language read-only investigation lane using gemini-3.1-pro-preview; Gemini fast survey is explicitly low-authority mechanical scan/summary work."
       "Gemini is initially read-only: research, review, context-pack, and Lisp compression advice may route there; scoped write/commit work stays on Claude until a separate Gemini write smoke passes."
       "Read-only Gemini pool workers MUST project to Gemini CLI `--approval-mode plan --policy .missiond/v3/policies/gemini-readonly-policy.toml`; workstation-pool registration MUST NOT set dangerously_skip_permissions/YOLO for any worker with :write-allowed false, and the policy MUST deny subagent delegation and write/shell tools."
       "Autopilot unassigned BoardTasks select from workstation-pool by task class before considering any legacy slot; old slots.yaml Sonnet entries are not generic coding candidates."
       "mission_compute_slot action=list must expose workstation_pool with runtime slot presence and idle/busy/stopped status."
       "Supervisor patrol (slot-supervisor) is gated on V3 workstation-pool / runtime-config registration; absent a supervisor worker entry the patrol stays inert and MUST NOT call ensure_memory_slot_by_id, so the legacy 'Memory slot not configured in slots.yaml' warning cannot fire."
       "V3 workstation-pool (plus startup-slots) is authoritative for dispatchable slots; mission_compute_slot list MUST tag any static slot whose id is not in the V3 projection as legacy=true and dispatchable=false (or split it into legacy_static_slots) so retired Sonnet entries (autopilot/topology-guardian/extraction-worker/delta-validator/...) cannot resurface as candidates."
       "mission_compute_slot list status MUST derive from PTYManager (state.pty.get_status) for every slot it surfaces, so it cannot contradict mission_pty_status for V3 pool slots; the SlotManager session_id field is only a fallback when no PTY status exists, and it MUST NOT report 'running' when no PTY is attached."]
    :checker "node scripts/check-v3-workstation-pool-isomorphism.mjs")

  (agent-interaction-policy
    :schema "missiond.agent-interaction-policy.v1"
    :desc "Prompt style is an interaction aid, not the safety boundary. Agents receive heuristic questions and prepared context; hard constraints live in BoardTask metadata, workflow Lisp, checker gates, and Rust runtime guards."
    (role resident-master
      :style heuristic-review
      :intake-question "关于用户提出的这个问题或需求，我还有哪些不知道的信息？这些未知信息分别应该从 SSOT、skill operational facts、项目代码、部署事实、事件总线、还是用户决策入口取得？"
      :intent-question "基于已补齐的证据，请判断用户此刻的真实意图、长期偏好或治理原则是什么；若判断成立，产出带 evidence_refs/confidence/supersession_scope 的 intent_memory_candidate；高置信意图必须写入 memory:decision，低置信只进入 needs-review/candidate。"
      :prompt-question "请审视当前目标和 SSOT Lisp：颗粒度是否足够细？哪些架构可以更优雅？你还需要哪些证据、调查工位或 exact shard？"
      :required-output-fields [decision reasoning_summary unknowns inferred_user_intent intent_memory_candidate evidence_needed delegation_plan? next_question_or_action]
      :default-inputs [active-boardtask context-pack-path current-phase event-summary]
      :forbidden-default-inputs [kb board-backlog historical-conversation provider-durable-logs]
      :rule "resident-master must complete unknowns-first-intake, intent-inference, intent-memory-capture, review-question, and evidence-plan before investigation/implementation unless the active BoardTask explicitly declares exact-shard-ready=true. Intent memory capture records the judged intent with evidence and confidence: high-confidence stable intent is written through mission_kb_remember(category=memory:decision), while uncertain intent stays as needs-review/candidate artifact. This does not re-enable broad KB prompt preloading.")
    (role investigator-worker
      :style context-prepared-question
      :prompt-opening "请审视、比较、找缺口并给出建议；只使用给定 read_scope/context_pack_path 的证据。"
      :metadata [BoardTask-ID project_id context_pack_path read_scope write_scope must_not_touch acceptance completion_protocol]
      :output-contract [Findings Evidence Recommendations Verification]
      :rule "investigator workers are read-only by default and produce context-pack evidence, not implementation patches or raw KB/log dumps.")
    (role implementer-worker
      :style accepted-shard-question
      :prompt-opening "基于已接受 shard 和上下文，请完成这个最小同构改动；保持现有行为，只在 declared write_scope 内修改。"
      :metadata [BoardTask-ID project_id context_pack_path read_scope write_scope must_not_touch acceptance completion_protocol]
      :output-contract [Changed-Files Acceptance-Evidence Residual-Risk]
      :rule "implementer workers may write only declared write_scope; exact shard ownership is enforced by task_delegate/Autopilot, not by prompt prose.")
    (role deterministic-llm-tool
      :style precise-instruction
      :rule "non-agent deterministic LLM calls may use exact prompts because they do not autonomously dispatch tools or workers.")
    :runtime-invariants
      ["Prompt text MUST NOT be the primary safety boundary; read/write scope, acceptance, pool routing, and closure policy are structured BoardTask/runtime fields."
       "Worker prompts MUST NOT prepend KB/Skill/history/provider-log context unless an explicit memory-audit workflow opts in."
       "M6 workflows MUST materialize context-pack artifacts with questions, hypotheses, evidence_needed, findings, design_options, and accepted_shards before implementation shards."
       "Direct implementation from an initial objective is allowed only when exact-shard-ready=true is explicitly present in the BoardTask metadata or description."]
    :checker "node scripts/check-v3-workflow-isomorphism.mjs")

  (codex-boot-context-policy
    :schema "missiond.codex-boot-context-policy.v1"
    :purpose "Every resident Codex, Codex worker, or external Codex handoff should start from a small validated capsule instead of inheriting a giant historical chat or hoping the agent reads a repo note."
    :capsule ".missiond/v3/evidence/codex-boot-context.lisp"
    :mcp mission_context_boot
    :layers
      ((layer L0-always-on :source ".missiond/v3/evidence/codex-boot-context.lisp" :rule "Always inject the shared collaboration protocol: Lisp SSOT, intent->plan->work-order, unknowns-first grounding, exact shard/write lease, and durable evidence completion.")
       (layer L1-current-task :source [mission_master_status BoardTask work-order] :rule "Load only active objective, BoardTask/work-order id, project_id, context_pack_path, accepted_shard_id, and checkpoint.")
       (layer L2-grounded-facts :source mission_context_gather :rule "Query SSOT/project registry/skill evidence/active memory/infra/tool directory only for explicit unknowns.")
       (layer L3-cold-evidence :source [raw-conversations historical-board provider-logs runtime-reports] :rule "Cold evidence is opt-in for audit/debug and must not be startup prompt preload."))
    :rules
      ["Boot capsule is a compact contract and MUST NOT contain secrets, raw provider logs, bulk chat history, or unreviewed KB dumps."
       "mission_context_boot is the public retrieval surface for external new conversations and resident/worker startup."
       "Every confirmed durable user intent that should survive a fresh conversation becomes an intent_memory_candidate, then active memory only after review or high-confidence evidence."]
    :checker "node scripts/check-v3-codex-boot-context-isomorphism.mjs")

  (skill-edit-delegation-policy
    :schema "missiond.skill-edit-delegation-policy.v1"
    :purpose "Keep ClaudeCode/Codex/Gemini skill files as operational knowledge managed by the workstation that understands that ecosystem best."
    :authority
      ((reader resident-master)
       (planner mission_context_gather)
       (editor claude-code-skill-maintainer)
       (reviewer codex-or-resident-master))
    :rules
      ["Codex/resident-master may read skill files as evidence through mission_skill_context or skill evidence artifacts."
       "Mutating skill files under ~/.claude/skills, ~/.codex/skills, or project skill directories MUST be represented as a BoardTask/work-order and delegated to a ClaudeCode skill-maintainer or deploy-ops lane."
       "Skill edits require context_pack_path, read_scope, write_scope, completion protocol, and a task-result-artifact; direct local edits by the resident master are not an accepted path."
       "When a user asks for skill changes, first gather related skill evidence, then create an intent.lisp/plan.lisp work-order or exact shard for ClaudeCode."]
    :checker "node scripts/check-v3-memory-kb-isomorphism.mjs")

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
      :refactor-rule "MissionD local EventBus remains the low-latency agent/Board/slot/workflow control bus. xjp-eventhub is the durable cross-service event backbone; MissionD syncs selected local events through an outbound spool and can continue local orchestration when xjp-eventhub is offline.")
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
	      ((tier t1 :source [provider-jsonl codex-sqlite claude-jsonl gemini-chat-file] :use "durable final/progress facts")
	       (tier t2 :source [missiond-event-bus BoardTask-lifecycle mission_execution] :use "causal workflow state")
	       (tier t3 :source [provider-aware-pty-recognition screen-buffer] :use "diagnostic state only; never sole completion authority"))
	    :pty-retention
	      (:ttl-days 1
	       :scope [screen-buffer screenshots pty-log-files slot-last-responses]
	       :rule "PTY content is transient diagnostic evidence only. MissionD keeps provider JSONL/Codex sqlite/Gemini chat files as durable logs, but PTY screen buffers, screenshots, pty-*.log files, and slot_last_responses MUST be treated as short-lived cache with a one-day retention window. Retention cleanup MUST be able to write a delete manifest so applied file/database removal remains reviewable and reversible by evidence.")
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
         (step s3 :logic "reconcile Board open tasks against provider JSONL/Codex sqlite/Gemini chat files")
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
               :dedupe-key "lisp-code-sync:<project>:<path-hash>"
               :rule "Lisp/checker edits under .missiond are observed through EventBus, compiled/checked immediately, runtime report paths are ignored, unchanged content fingerprints are suppressed, debounce repeated path events, retention/GC bounds report volume, and only failing gates create visible BoardTasks.")
      :core
        ((step s1 :logic "watch active ProjectRegistry .missiond directories recursively for .lisp and .mjs changes")
         (step s2 :logic "publish each relevant filesystem change as SystemEvent::ConfigChanged; sync processing subscribes to EventBus, rechecks path/content fingerprint before expensive gates, and does not bypass EventBus")
         (step s3 :logic "resolve project by longest-prefix ProjectRegistry match")
         (step s4 :logic "for missiond run compile-v3-runtime then check-v3-code-isomorphism-complete; for external projects run .missiond/check.sh when present")
         (step s5 :logic "write .missiond/v3/runtime/lisp-code-sync/<timestamp>-<path-hash>.report.lisp with synced/needs-sync/observed-only status")
         (step s6 :logic "ignore .missiond/v3/runtime/** and .missiond/runtime-state/** before EventBus publication so self-generated reports cannot recurse")
         (step s7 :logic "suppress unchanged content fingerprints at both watcher publication and subscription consumption, debounce repeated path events, and apply report retention/GC to keep the sync loop bounded")
         (step s8 :logic "on failed code-isomorphism create or reuse one visible BoardTask lisp-code-sync:<project>:<path-hash> that requires evidence-plan and exact accepted shard before code mutation")
         (step s9 :logic "Autopilot revalidates lisp-code-sync runtime report BoardTasks before slot selection; tasks that point at .missiond/v3/runtime/lisp-code-sync/** are closed as resolved_by_runtime_fix/stale_evidence and never sent to PTY")
         (step s10 :logic "expose lispCodeSync status in mission_master_status so the resident master and frontend can see the live Lisp->code loop"))
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
       "mission_router_chat default model and max_tokens MUST project from router-runtime-policy; explicit caller model/max_tokens still wins."
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

  (project-registry-policy
    :desc "Lisp-owned project registry defaults for intent discovery and universe import."
    :intent-path-candidates [".missiond/intent.lisp" ".jarvis/intent.lisp" "intent.lisp"]
    :default-universe-manifest "/Users/jinchen/Projects/universe.intent.lisp"
    :env-overrides [UNIVERSE_MANIFEST]
    :invariants
      ["mission_project init/import_universe/survey MUST project intent-path candidates from project-registry-policy."
       "mission_project import_universe MUST project its default manifest from project-registry-policy; UNIVERSE_MANIFEST is only an explicit override."
       "A real MissionD project with .missiond but no project-registry-policy MUST return V3_BLUEPRINT_CONFIG_ERROR rather than silently using embedded defaults."])

  (eventbridge-policy
    :schema "missiond.eventbridge-policy.v1"
    :envelope missiond.event-envelope.v1
    :fields [event_id source project_id service_id event_kind subject correlation_id trace_id occurred_at observed_at authority schema_version payload privacy_class]
    :taxonomy [deploy_created build_started build_succeeded build_failed deploy_started deploy_succeeded deploy_failed smoke_succeeded smoke_failed rollback_started rollback_succeeded rollback_failed agent_heartbeat agent_offline agent_update_started agent_update_succeeded agent_update_failed provenance_changed usage_burst provider_error_burst provider_auth_failure_burst quota_exhaustion]
    :rule "MissionD remains the local orchestrator and EventBridge. Cloud services send durable provider events through typed webhooks; MissionD stores them as SystemEvent::ExternalServiceEvent with idempotent event_id dedupe. PTY remains diagnostic only."
    :invariants
      ["External deploy events MUST enter through /webhooks/deploy-center-event or /webhooks/service-event with X-MissionD-Webhook-Token when MISSIOND_EXTERNAL_WEBHOOK_TOKEN is configured."
       "Deploy-center event envelopes MUST carry stable event_id values derived from deploy-center durable rows; MissionD MUST reject deploy-center events without event_id."
       "mission_timeline(action=wait, domain=system) MUST support serviceId, eventKind, projectId, and correlationId predicates for deployment events."
       "ExternalServiceEvent append MUST use deterministic dedupe by service_id + event_id."])

  (eventhub-service-contract
    :schema "missiond.eventhub-service-contract.v1"
    :service-id xjp-eventhub
    :purpose "Extract cross-service durable event storage, waits, subscriptions, cursors, and replay into an XJP backend service while preserving MissionD's local low-latency EventBus for agent/Board/slot/workflow control."
    :ownership
      ((owner missiond-local-eventbus
        :owns [agent-events board-events slot-events workflow-events pty-diagnostics local-wakeups]
        :rule "Local orchestration must continue when xjp-eventhub is unavailable; local events are spooled for later outbound relay when configured.")
       (owner xjp-eventhub
        :owns [durable-event-envelope stream-cursors subscriptions wait-predicates dead-letter-replay cross-service-events]
        :runtime-env [MISSIOND_EVENTHUB_URL MISSIOND_EVENTHUB_TOKEN]
        :rule "xjp-eventhub is the cloud/service event backbone for deploy-center, auth, router, timeline, and selected MissionD local events.")
       (owner deploy-center
        :owns [deployment-provenance deploy-agent-events rollout-events]
        :rule "deploy-center remains deployment fact authority; eventhub stores and distributes its emitted facts but does not infer release state."))
    :event-envelope
      (schema missiond.event-envelope.v1
        :fields [event_id source project_id service_id event_kind subject correlation_id trace_id occurred_at observed_at authority schema_version payload privacy_class]
        :idempotency [source event_id]
        :privacy-classes [public internal private secret-redacted])
    :functions
      ((function eventhub-service-boundary
         :entry [MissionD-local-EventBus deploy-center-events auth-events router-events timeline-events]
         :core ((step s1 :logic "classify event as local-control, cross-service, or diagnostic")
                (step s2 :logic "store local-control events in MissionD local bus first")
                (step s3 :logic "spool selected events to xjp-eventhub with source/event_id idempotency")
                (step s4 :logic "preserve MissionD offline operation when xjp-eventhub is unavailable"))
         :egress [local-event-bus outbound-spool xjp-eventhub-event])
       (function local-event-spool
         :entry [EventBus-publish outbound-relay-tick]
         :core ((step s1 :logic "persist selected local events with cursor and retry metadata")
                (step s2 :logic "redact payload fields by privacy_class before relay")
                (step s3 :logic "relay to xjp-eventhub when endpoint and token are configured")
                (step s4 :logic "mark delivered, retryable, or dead-letter without blocking local MissionD workflows"))
         :egress [spool-row relay-diagnostic])
       (function eventhub-wait-contract
         :entry [mission_timeline.wait eventhub.wait]
         :core ((step s1 :logic "resolve predicate over project_id/service_id/event_kind/correlation_id/trace_id")
                (step s2 :logic "prefer local EventBus for local-control predicates")
                (step s3 :logic "use xjp-eventhub for cross-service predicates when configured")
                (step s4 :logic "fall back to bounded local event_log polling with visible diagnostic only when eventhub is unavailable"))
         :egress [wait-result timeout-diagnostic]))
    :runtime-projection [xjp-eventhub service-runtime-universe eventbridge local-event-spool mission_timeline.wait mission_timeline.eventhub_status mission_timeline.eventhub_query mission_timeline.eventhub_append]
    :checker "node scripts/check-v3-service-extraction-isomorphism.mjs")

  (provider-runtime-bringup-contract
    :schema "missiond.provider-runtime-bringup.v1"
    :purpose "Make local xjp-memory and xjp-eventhub provider runtime reproducible instead of relying on one-off LaunchAgent edits."
    :script "scripts/manage-local-providers.sh"
    :services
      ((provider xjp-memory
         :label "com.xjp.memory.provider"
         :url "http://127.0.0.1:8091"
         :database "xjp_memory"
         :storage postgres-durable
         :missiond-env [MISSIOND_MEMORY_PROVIDER_URL MISSIOND_MEMORY_PROVIDER_MODE])
       (provider xjp-eventhub
         :label "com.xjp.eventhub.provider"
         :url "http://127.0.0.1:8092"
         :database "xjp_eventhub"
         :storage postgres-durable
         :missiond-env [MISSIOND_EVENTHUB_URL MISSIOND_EVENTHUB_MODE]))
    :functions
      ((function local-provider-launchd
         :entry [developer-local-install scripts.manage-local-providers.install launchd]
         :core ((step s1 :logic "build xjp-memory and xjp-eventhub from the canonical XJP monorepo")
                (step s2 :logic "ensure local Postgres databases xjp_memory and xjp_eventhub exist")
                (step s3 :logic "write LaunchAgent plists for com.xjp.memory.provider and com.xjp.eventhub.provider")
                (step s4 :logic "wire MissionD LaunchAgent provider env to 127.0.0.1 service URLs")
                (step s5 :logic "bootstrap providers and MissionD, then smoke provider status endpoints"))
         :egress [launchd-plist missiond-provider-env provider-smoke-report])
       (function local-provider-smoke
         :entry [scripts.manage-local-providers.smoke mission_memory.provider_status mission_timeline.eventhub_status]
         :core ((step s1 :logic "query /v1/memory/provider_status and require postgres-durable storage")
                (step s2 :logic "query /v1/eventhub/status and require postgres-durable storage")
                (step s3 :logic "surface provider diagnostics through MCP instead of assuming local compatibility mode"))
         :egress [provider-status diagnostic]))
    :invariants
      ["Local provider enablement MUST be reproducible through scripts/manage-local-providers.sh install, not manual plist editing."
       "The script MUST not store secrets; provider tokens remain in secret-store/env and may be injected separately."
       "MissionD must continue to support null/local compatibility providers when xjp-memory or xjp-eventhub are not configured."]
    :checker "node scripts/check-v3-service-extraction-isomorphism.mjs")

  (deployment-event-ingest
    :schema "missiond.deployment-event-ingest.v1"
    :entry [/webhooks/deploy-center-event mission_timeline.wait deployment-event-response.workflow]
    :core ((step s1 :logic "validate optional MissionD webhook token and parse missiond.event-envelope.v1")
           (step s2 :logic "preserve event identity and project/correlation fields under payload._envelope")
           (step s3 :logic "publish SystemEvent::ExternalServiceEvent through EventBus with service/event dedupe")
           (step s4 :logic "allow master, Autopilot, and deploy-ops workflows to wait by service_id, event_kind, project_id, and correlation_id")
           (step s5 :logic "create BoardTask suggestions for deploy/smoke/agent-offline/agent-update failures; attach break-glass runbook refs when the deploy agent is unreachable, but never auto rollback, SSH, DNS mutate, secret mutate, or production-deploy without deploy-center policy or user approval"))
    :egress [ExternalServiceEvent mission_timeline.wait deployment-ops-BoardTask]
    :surfaces [eventbridge project-registry])

  (router-usage-event-ingest
    :schema "missiond.router-usage-event-ingest.v1"
    :entry [/webhooks/service-event mission_timeline.wait router-usage-alert]
    :core ((step s1 :logic "accept router service-event envelopes for usage_burst, provider_error_burst, provider_auth_failure_burst, and quota_exhaustion without treating PTY as evidence")
           (step s2 :logic "preserve caller attribution fields project_id, service_id, provider, model, route, request_id, tenant_id hash, status_code, and error class")
           (step s3 :logic "dedupe burst alerts by service_id + provider + model + window_start + event_kind")
           (step s4 :logic "surface repeated provider/auth failures to Board as diagnostic incidents and do not retry hidden translation or background LLM work")
           (step s5 :logic "allow mission_timeline waits and master-control observers to react to router anomalies from durable events"))
    :egress [ExternalServiceEvent router-usage-diagnostic router-ops-BoardTask]
    :surfaces [eventbridge router-policy])

  (deployment-change-classification-policy
    :schema "missiond.deployment-change-classification-policy.v1"
    :entry [git-diff deploy-center.provenance ci-dispatcher xjp-workspace-ssot deploy-workflow-validation]
    :core ((step s1 :logic "classify a change set before any deployment fanout: service-runtime-change, workflow-only-change, ssot-checker-only, deploy-config-change, secret-dns-change, or unknown")
           (step s2 :logic "service-runtime-change may trigger the affected service deployment through deploy-center after normal provenance/smoke gates")
           (step s3 :logic "workflow-only changes, reusable deploy workflow changes, checker-only changes, and SSOT-only changes run validation-only workflows and must not fan out production service deployments")
           (step s4 :logic "deploy-config-change requires deploy-center provenance plus explicit rollout intent; secret-dns-change requires Decision Inbox or deploy-center policy before mutation")
           (step s5 :logic "unknown classification creates a diagnostic BoardTask/context-pack instead of guessing or dispatching deploy-ops workers"))
    :egress [deploy-intent validation-only-run deploy-rollout-suppression deploy-diagnostic-BoardTask]
    :surfaces [".missiond/workflows/deployment-event-response.lisp" "xjp-backend:.missiond/backend/xiaojinpro-backend-blueprint.lisp" "xjp-backend:.github/workflows/deploy-workflow-validation.yml"]
    :rule "Deployment work begins with change classification. A CI/workflow/tooling patch is not service runtime evidence and must be validated without causing broad production fanout. MissionD may observe and create diagnostics, but deploy-center remains the deployment fact authority.")

  (m6-deployment-confirmation
    :schema "missiond.m6-deployment-confirmation.v1"
    :entry [project-maturity-registry service-runtime-universe deploy-center.status deploy-center.provenance]
    :core ((step s1 :logic "select projects whose project-maturity-registry current level is M6")
           (step s2 :logic "map each project to deploy-center service slug(s), for example auth→xjp-auth-center, deploy-center→xjp-deploy-center, router→xjp-router, pcea→pcea/pcea-api/pcea-video-vault")
           (step s3 :logic "query deploy-center /api/deploy/status and provenance surfaces; classify deployed-current, deployed-stale, not-confirmed, or deployed-unknown")
           (step s4 :logic "compare deployed commit to local service paths where the project lives in a git checkout; do not mark a service current when service-relevant files changed after the deployed commit")
           (step s5 :logic "distinguish CI/build/push success, deploy-center notify HTTP 200, deploy-center provenance, and service smoke; only provenance plus smoke can close deployment confirmation")
           (step s6 :logic "classify digest_resolution_failed, reported_digest_missing, runner_queued, build_cache_unavailable, and provenance_partial as typed diagnostics rather than burying them in free text")
           (step s7 :logic "order rollout through deploy-center before dependent services and emit a machine-readable deployment gap report"))
    :egress [m6-deployment-status-json deploy-ops-BoardTask m6-rollout-report]
    :surfaces ["scripts/check-m6-deployment-status.mjs" ".missiond/workflows/m6-deployment-rollout.lisp" ".missiond/workflows/pcea-deployment-rollout.lisp" "scripts/check-v3-project-registry-isomorphism.mjs"]
    :diagnostics [runner_queued build_cache_unavailable digest_resolution_failed reported_digest_missing provenance_partial]
    :rule "M6 maturity is not deployment evidence. Production deployment confirmation must come from deploy-center status/provenance and service smoke, with curl/git/GitHub probes only as diagnostics. Build-cache accelerators such as sccache/kellnr are performance aids and must not become implicit release blockers.")

  (deployment-evidence-preflight
    :schema "missiond.deployment-evidence-preflight.v1"
    :entry [m6-deployment-rollout pcea-deployment-rollout mission_infra_query skill-runtime deploy-center.provenance]
    :core ((step s1 :logic "resolve project_id to MissionD Universe identity, project deployment SSOT, and deploy-center slug(s)")
           (step s2 :logic "collect skill-derived deployment evidence with include_kb=false, preserving source_skill/source_path/source_line and redacting credential-like values")
           (step s3 :logic "query deploy-center runtime/provenance and compare with skill evidence for host, agent, script, artifact, health, and rollback facts")
           (step s4 :logic "verify deploy-center pull-mode executor claim dependencies: deploy_executors.api_key_ref must resolve DEPLOY_AGENT_API_KEY from Secret Store on gcp-runtime before agent-offline or script-failure conclusions are trusted")
           (step s5 :logic "if skill evidence, deploy-center facts, Secret Store dependency health, and project SSOT disagree, create a drift diagnostic/Decision item and do not let deploy workers guess host, login path, script path, or agent project")
           (step s6 :logic "materialize a deploy context-pack for deploy-ops workers containing only reconciled facts, remaining unknowns, smoke commands, dependency-health evidence, and approval boundaries"))
    :egress [deploy-context-pack runtime-fact-drift deploy-ops-BoardTask]
    :surfaces [".missiond/workflows/m6-deployment-rollout.lisp" ".missiond/workflows/pcea-deployment-rollout.lisp" "crates/missiond-daemon/src/bus/v2_subscribers.rs" "scripts/check-v3-workflow-isomorphism.mjs"]
    :rule "Every deployment task must perform deployment-evidence-preflight before action. Skills are evidence and operational guidance; deploy-center provenance is deployment authority; MissionD orchestrates and records the decision path."
    :dependency-rule "Secret Store is the credential authority for deploy-center executor claim auth. Since 2026-05-11 its production runtime is ss.xiaojinpro.top on the GCP xjp-backend VM, not ClawCloud. If Secret Store is unreachable, classify deploy-blocked-by-secret-store against gcp-runtime/Caddy/docker/xjp-postgres health and surface namespace/key refs only; never expose credential values.")

  (project-identity-contract
    :schema "missiond.project-identity-contract.v1"
    :fields [project_id canonical_root repo_remote ssot_paths deploy_center_slug forge_project_name service_ids aliases status]
    :rule "MissionD is project identity and SSOT registry authority; deploy-center is deployment fact authority; Forge is component/pattern/reality catalog authority."
    :reconcile-action mission_project.reconcile
    :invariants
      ["MissionD Universe owns canonical project ids, roots, SSOT paths, maturity, Board links, and workstation dispatch."
       "deploy-center owns deployment targets, runtime location, release provenance, deploy agents, and executor state."
       "Forge owns component/pattern catalog, code reality mirror, and Universe DAG recommendations; Forge-only references are not deployable unless MissionD registers them."
       "Historical aliases such as xjp-deploy-center MUST NOT become active project roots."])

  (registry-authority-map
    :schema "missiond.registry-authority-map.v1"
    :authorities ((missiond :owns [project-identity ssot-paths maturity board-workstation-scheduling])
                  (deploy-center :owns [deployment-targets runtime-location release-provenance agent-executor-state])
                  (forge :owns [component-catalog pattern-catalog code-reality-mirror universe-dag-recommendations]))
    :workflow project-registry-reconciliation
    :rule "Registry reconciliation reads MissionD, deploy-center, and Forge facts, reports missing_in_*, alias_conflict, root_mismatch, and deploy_fact_missing, and never silently overwrites identities.")

  (infrastructure-universe
    :schema "missiond.infrastructure-universe.v1"
    :rule "Servers, runtime targets, deployment locations, agent/executor facts, and skill-derived ops knowledge are first-class governance objects. MissionD owns the Universe summary and dispatch policy; deploy-center owns verified runtime/deployment facts; secret-store owns credential values; skills are evidence only."
    (runtime-target-contract
      :fields [target_id aliases kind environment owner_authority capabilities deploy_center_executor agent_url service_ids network_profile lan_group artifact_lanes evidence_refs freshness]
      :invariants ["Runtime targets promoted from skills MUST be marked unverified until deploy-center or an approved probe confirms them."
                   "MissionD workers encountering an unknown server MUST query mission_infra_query(action=skill_evidence|reconcile) before guessing login paths or deployment authority."
                   "Runtime facts from deploy-center provenance override local skill notes; MissionD never silently overwrites conflicts."])
    (credential-ref-contract
      :fields [secret_ref namespace key_name purpose required_capability]
      :invariants ["Lisp, Board notes, context packs, and skills MUST NOT become active stores for login passwords, API keys, Cloudflare tokens, or SSH secrets."
                   "mission_infra_query(action=credential_refs) returns secret refs and availability only; it never returns credential values."
                   "Credential-like skill lines are migration evidence and must be redacted before entering worker context."
                   "Provider account credentials such as Aliyun AccessKey are stored once as account-level secrets; capability targets such as DNS, OSS, ECS, or billing reference the account credential instead of duplicating narrower key names."])
    (skill-evidence-contract
      :fields [source_skill source_path source_line confidence last_verified_at promote_to credential_inline_risk excerpt]
      :rule "Skills are operational guidance and discovery evidence. A skill fact becomes active runtime truth only after reconcile promotes it into deploy-center runtime inventory or MissionD Universe with a source reference.")
    (break-glass-runbook-contract
      :fields [runbook_id target_id service_id source_skill evidence_refs allowed_actions forbidden_actions credential_refs approval_required freshness]
      :rule "Manual ECS/SSH/operator fallback is a break-glass runbook, not the primary deploy path. It is attached to deploy-ops tasks only when deploy-center reports agent_offline/agent_update_failed or provenance cannot be obtained, and it must reference secret-store credential refs instead of inline secrets.")
    (read-only-remote-diagnostic-contract
      :fields [profile_id target_id service_id authority read_only allowed_operations forbidden_operations credential_refs event_sink artifact_sink]
      :profiles [deploy_provenance_snapshot container_inventory dependency_manifest_scan supply_chain_ioc_scan]
      :invariants ["Remote diagnostic work MUST resolve a deploy-center read-only diagnostic profile before touching agent endpoints. MissionD may list profile requirements with mission_infra_query(action=diagnostic_profiles), but it MUST NOT guess deploy-agent API keys or run raw agent exec for diagnostics."
                   "Allowed profile operations are restricted to deploy provenance snapshots, container inventory without env, dependency manifest file scans, and supply-chain IoC grep over already-present files. npm/pnpm/yarn/pip install, Python/Node import, lifecycle scripts, container env reads, mutating docker/system commands, and secret dumps are forbidden."
                   "Diagnostic output is stored as task-result-artifact or ExternalServiceEvent evidence; PTY/log output is a projection only and credential values are never returned."])
    (target-network-profile-contract
      :fields [profile_id allowed_outbound forbidden_outbound allowed_transfer_stores build_runtime_candidates target_side_build_allowed diagnostics]
      :invariants ["CN restricted targets such as Aliyun ECS and Synology/domestic-only VMs MUST NOT depend on target-side GitHub, GHCR, Docker Hub, or source builds."
                   "Privatecloud Ubuntu 10900KF, Windows 12900KF, and Synology VM share xjp-zibo-lan and may be used as build/cache/jump evidence when their agent/credential refs are healthy."
                   "Managed Mac nodes with enough local CPU such as rickyhq-macmini-m4 SHOULD receive source through the XJP native codebase lane and build on target; direct binary scp is a break-glass bootstrap path only."
                   "If a deploy worker sees a restricted target configured for GHCR/GitHub direct pull, it must create deployment-lane-mismatch instead of retrying network calls."])
    (artifact-delivery-lane-contract
      :fields [lane_id source_commit builder_id transfer_store target_runtime artifact_sha256 target_digest reported_digest rollback_artifact smoke_evidence]
      :lanes [cloud-registry-lane cn-oss-bundle-lane gitee-source-mirror-lane macmini-codebase-local-build-lane manual-break-glass-lane]
      :invariants ["cn-oss-bundle-lane means approved builder -> Aliyun OSS -> ECS internal download -> deploy-agent run -> reported digest; ECS must not build or pull GHCR as the normal path."
                   "macmini-codebase-local-build-lane means MissionD/deploy-center sync source and workflow definition to the managed Mac node, the node builds locally, signs/installs into its own ~/.xjp-mission release path, and reports build/test/health provenance. This lane is preferred after bootstrap because it avoids brittle large binary transfer and proves the managed node can rebuild itself."
                   "gitee-source-mirror-lane is source/control evidence only unless paired with a builder and artifact lane."
                   "manual-break-glass-lane requires approval and post-action provenance."])
    (agent-offline-response-policy
      :entry [deploy-center.agent_heartbeat deploy-center.agent_update_failed deployment-event-response mission_infra_query.skill_evidence mission_infra_query.diagnostic_profiles]
      :core ((step s1 :logic "when deploy-center emits agent_offline or repeated heartbeat/update failure, MissionD creates or updates one deploy-ops incident keyed by target_id/service_id/root_cause_key")
             (step s2 :logic "MissionD queries runtime target inventory and skill evidence for break-glass runbook refs such as PCEA ECS jump-host/OSS/deploy.sh facts, redacting any credential-like line")
             (step s3 :logic "MissionD first asks deploy-center for read-only diagnostic profiles such as deploy_provenance_snapshot, container_inventory, dependency_manifest_scan, and supply_chain_ioc_scan; unavailable credentials become Decision Inbox or secret-store binding gaps, not guessed raw agent calls")
             (step s4 :logic "resident master presents options: wait for agent recovery, trigger deploy-center self-update, run an approved read-only diagnostic profile, or use approved manual runbook; manual actions require explicit approval and deploy-ops worker context")
             (step s5 :logic "if a diagnostic profile or manual runbook is used, write evidence back to deploy-center/MissionD as provenance gap remediation instead of leaving an untracked shell operation"))
      :egress [deploy-ops-BoardTask break-glass-context-pack Decision-Inbox deploy-center-provenance-gap]
      :surfaces [".missiond/workflows/deployment-event-response.lisp" ".missiond/workflows/m6-deployment-rollout.lisp" "mission_infra_query(action=skill_evidence|credential_refs|diagnostic_profiles)"])
    (runtime-authority-map
      :authorities ((missiond :owns [project-identity universe-summary dispatch-policy eventbridge])
                    (deploy-center :owns [runtime-target-inventory executor-inventory service-deploy-location agent-heartbeat-provenance release-provenance])
                    (secret-store :owns [credential-values credential-rotation credential-availability])
                    (skills :owns [operational-guidance evidence-source workflow-procedure])
                    (forge :owns [component-catalog pattern-catalog code-reality-mirror])))
    (cloud-ops-delegation-policy
      :entry [operator-request mission_infra_query.credential_refs mission_infra_query.skill_evidence deployment-event-response m6-deployment-rollout]
      :core ((step s1 :logic "classify credential rotation, DNS changes, cloud account inventory, OSS/ECS setup, and deploy-agent recovery as cloud/deploy ops rather than generic coding work")
             (step s2 :logic "resident master builds a redacted context pack with target_id, credential_ref availability, skill evidence refs, intended mutation, rollback/verification command, and approval boundary")
             (step s3 :logic "delegate operational execution to the explicit claude-code-deploy-ops lane; Codex/resident master supervises, validates evidence, and updates SSOT/evidence, but does not perform routine shell/cloud console operations itself")
             (step s4 :logic "after one-off operation succeeds, promote reusable procedure into deploy-center/MissionD workflow evidence and keep only secret refs, not secret values"))
      :egress [deploy-ops-BoardTask cloud-ops-context-pack task-result-artifact ssot-evidence-update]
      :surfaces [".missiond/workflows/m6-deployment-rollout.lisp" ".missiond/workflows/deployment-event-response.lisp" "mission_infra_query(action=credential_refs|skill_evidence)"])
    (runtime-target :target_id gcp-runtime
      :aliases [gcp-production]
      :kind cloud-runtime
      :environment production
      :owner_authority deploy-center
      :capabilities [auth router deploy-center secret-store credential-vault caddy-reverse-proxy production-runtime google-cloud-storage global-object-store]
      :service_ids [auth router deploy-center secret-store global-object-store]
      :public_domain "ss.xiaojinpro.top"
      :public_ip "34.104.147.118"
      :credential_refs [secret-store://cloud/gcp/deploy-center-runtime secret-store://deploy-agent/gcp/DEPLOY_AGENT_API_KEY secret-store://secret-store/cloudflare/CLOUDFLARE_DNS_TOKEN]
      :diagnostic_profiles [deploy_provenance_snapshot container_inventory dependency_manifest_scan supply_chain_ioc_scan]
      :freshness verified-2026-05-11
      :evidence_refs [service-runtime-universe deploy-center-provenance secret-store-gcp-migration-20260511 gcp-global-object-store-20260513])
    (runtime-target :target_id aliyun-account
      :aliases [aliyun-global aliyun-cloud-account]
      :kind cloud-account
      :environment cn-production
      :owner_authority deploy-center
      :capabilities [alidns oss ecs ram cloud-account-inventory domain-record-inventory domain-record-upsert object-storage-bucket-management ecs-runtime-management]
      :service_ids [long-image-service pcea secret-store-cn deploy-center-cn]
      :freshness credential-rotated-and-dns-read-verified-2026-05-13
      :credential_refs [secret-store://aliyun-global/ALIYUN_ACCESS_KEY_ID secret-store://aliyun-global/ALIYUN_ACCESS_KEY_SECRET]
      :evidence_refs [aliyun-global-access-key-rotation-20260513 skill:aliyun])
    (runtime-target :target_id aliyun-dns
      :aliases [aliyun-alidns changtu-pro-dns]
      :kind dns-provider
      :environment cn-production
      :owner_authority deploy-center
      :capabilities [domain-record-inventory domain-record-upsert changtu-pro-dns]
      :service_ids [long-image-service pcea]
      :freshness dns-read-verified-2026-05-13
      :credential_refs [secret-store://aliyun-global/ALIYUN_ACCESS_KEY_ID secret-store://aliyun-global/ALIYUN_ACCESS_KEY_SECRET]
      :evidence_refs [aliyun-global-access-key-rotation-20260513 changtu-pro-deployment-and-payment-boundary-20260513 skill:aliyun])
    (runtime-target :target_id ecs-pcea
      :aliases [pcea-ecs]
      :kind cloud-vm
      :environment production
      :owner_authority deploy-center
      :capabilities [pcea deploy-agent runtime secret-store-cn long-image-service]
      :service_ids [pcea secret-store-cn long-image-service]
      :network_profile ecs-cn-restricted
      :artifact_lanes [cn-oss-bundle-lane gitee-source-mirror-lane manual-break-glass-lane]
      :freshness verified-runtime-smoke-2026-05-15
      :runtime_facts (instance_id "i-uf6641fl52xo7ukf7kgl"
                      instance_name "iZuf6641fl52xo7ukf7kglZ"
                      public_ip "106.15.2.17"
                      zone "cn-shanghai-e"
                      agent_version "10.7.2"
                      current_containers [pcea-app pcea-api pcea-postgres secret-store-cn-app long-image-service]
                      runtime_smoke [secret-store-cn-livez secret-store-cn-readyz long-image-public-icp-blocked long-image-host-header-stale]
                      public_domain_blocks [changtu-pro-icp])
      :break_glass_runbook_refs [skill:pcea#ssh skill:pcea#deploy skill:aliyun#ECS skill:deploy-ops#deploy-agent]
      :credential_refs [secret-store://deploy-agent/DEPLOY_AGENT_ECS_API_KEY secret-store://infra/aliyun-ecs/ssh]
      :diagnostic_profiles [deploy_provenance_snapshot container_inventory dependency_manifest_scan supply_chain_ioc_scan]
      :evidence_refs [skill:pcea skill:aliyun skill:deploy-ops secret-store-cn-ecs-deploy-20260513 secret-store-cn-runtime-verified-20260515 changtu-pro-cn-deployment-20260513])
    (runtime-target :target_id privatecloud-10900kf
      :aliases [privatecloud privatecloud-lan-192-168-1-20 ubuntu-10900kf]
      :kind local-lan-builder
      :environment local-lan
      :owner_authority deploy-center
      :deploy_center_executor privatecloud
      :agent_url privatecloud
      :capabilities [cn-build cache harbor github-runner deploy-agent domestic-jump]
      :service_ids []
      :network_profile privatecloud-build-lan
      :lan_group xjp-zibo-lan
      :artifact_lanes [cn-oss-bundle-lane gitee-source-mirror-lane]
      :freshness declared-2026-05-13-agent-offline
      :credential_refs [secret-store://deploy-agent/DEPLOY_AGENT_API_KEY]
      :evidence_refs [skill:private-cloud user-topology-20260513])
    (runtime-target :target_id privatecloud-hostvds
      :aliases [hostvds privatecloud]
      :kind vps-runtime
      :environment privatecloud
      :owner_authority deploy-center
      :capabilities [deploy tunnel runtime]
      :service_ids []
      :freshness unverified
      :evidence_refs [skill:missiond-memory skill:xjp-deploy-center])
    (runtime-target :target_id windows-12900kf
      :aliases [12900kf windows-runner]
      :kind windows-workstation
      :environment local-lan
      :owner_authority deploy-center
      :deploy_center_executor windows
      :agent_url windows
      :capabilities [gpu github-runner embedding rerank deploy-agent]
      :service_ids [router]
      :network_profile privatecloud-build-lan
      :lan_group xjp-zibo-lan
      :freshness skill-derived-unverified
      :credential_refs [secret-store://deploy-agent/windows-12900kf/agent-token]
      :evidence_refs [skill:windows-runner skill:missiond-model-routing])
    (runtime-target :target_id rickyhq-macmini-m4
      :aliases [rickyhqmac-mini macmini-managed-node macmini-missiond-worker]
      :kind managed-mac-node
      :environment local-lan
      :owner_authority missiond
      :deploy_center_executor macmini
      :agent_url rickyhqmac-mini
      :capabilities [missiond-daemon mission-mcp claude-code codex-cli gemini-cli local-rust-build codebase-runner local-blue-green]
      :service_ids [missiond]
      :network_profile mac-managed-node
      :artifact_lanes [macmini-codebase-local-build-lane manual-break-glass-lane]
      :freshness health-verified-2026-05-18
      :runtime_facts (hostname "RickyHQdeMac-mini.local"
                      user "rickyhq"
                      project_root "/Users/rickyhq/Projects/missiond"
                      runtime_root "/Users/rickyhq/.xjp-mission"
                      health "http://127.0.0.1:9120/health"
                      launchd_label "com.missiond.daemon"
                      local_build_capability true
                      bootstrap_note "direct binary transfer is allowed only for initial repair; steady state should use codebase sync plus local build")
      :credential_refs [secret-store://managed-node/rickyhq-macmini/ssh secret-store://managed-node/rickyhq-macmini/claude]
      :diagnostic_profiles [deploy_provenance_snapshot container_inventory dependency_manifest_scan supply_chain_ioc_scan]
      :evidence_refs [work-order:20260516-macmini-managed-node skill:rickyhqmac-mini])
    (runtime-target :target_id synology-astrill-gw
      :aliases [synology-vm astrill-gw domestic-jump]
      :kind local-lan-gateway
      :environment local-lan
      :owner_authority deploy-center
      :capabilities [domestic-jump network-gateway]
      :service_ids []
      :network_profile synology-cn-restricted
      :lan_group xjp-zibo-lan
      :artifact_lanes [manual-break-glass-lane]
      :freshness declared-2026-05-13-credential-ref-required
      :credential_refs [secret-store://infra/synology-astrill-gw/ssh]
      :evidence_refs [skill:astrill-gateway user-topology-20260513])
    (runtime-target :target_id bwg-vps
      :aliases [bwg model-tunnel]
      :kind vps-tunnel
      :environment relay
      :owner_authority deploy-center
      :capabilities [tunnel router-relay model-relay]
      :service_ids [router]
      :freshness skill-derived-unverified
      :credential_refs [secret-store://infra/bwg-vps/tunnel-ssh]
      :evidence_refs [skill:missiond-model-routing])
    (runtime-target :target_id privatecloud-lan-192-168-1-20
      :aliases [lan-infra harbor-cache]
      :kind local-lan-node
      :environment local-lan
      :owner_authority deploy-center
      :capabilities [cache harbor dns registry]
      :service_ids []
      :network_profile privatecloud-build-lan
      :lan_group xjp-zibo-lan
      :freshness skill-derived-unverified
      :evidence_refs [skill:xjp-deploy-center])
    :surfaces ["crates/missiond-daemon/src/handlers/sysinfra/infra.rs"
               "crates/missiond-daemon/src/handlers/knowledge/project/reconcile.rs"
               "crates/missiond-mcp/src/tools/sysinfra/infra.rs"
               "packages/board/src/app/api/infra/route.ts"
               "packages/board/src/components/SystemDashboard.tsx"
               "scripts/check-v3-infrastructure-universe-isomorphism.mjs"])

  (data-residency-universe
    :schema "missiond.data-residency-universe.v1"
    :purpose "Govern legal/technical data partitions for data-bearing projects. This is an architecture SSOT, not legal advice: it makes region identity, issuer, secrets, storage, payment, model routing, and cross-region egress explicit so MissionD can refuse ambiguous M6 releases."
    :research ".missiond/research/data-residency-universe-report-20260512.md"
    :rule "cn and global are hard partitions at the XJP platform layer. Applications such as PCEA and CUTHUB bind to xjp-cn or xjp-global instead of inventing separate auth/secret/payment/router stacks. global-eu is an operating zone inside xjp-global until a project explicitly declares a separate hard partition. Region routing uses explicit project/workspace selection plus account/payment signals; IP is a hint only."
    (data-region-partition-contract
      :partition-key project-or-workspace-id
      :partitions
        ((partition cn
           :boundary hard
           :authority "China mainland legal entity / ICP / local runtime"
           :identity-namespace cn
           :runtime "mainland cloud runtime; exact deploy-center target required before launch"
           :must-not-share [issuer signing-key kek payment-ledger object-storage vector-db prompt-corpus eventhub user-table])
         (partition global
           :boundary hard
           :authority "global legal entity / non-mainland runtime"
           :identity-namespace global
           :operating-zones [global-us global-eu]
           :must-not-share-with [cn])
         (operating-zone global-eu
           :parent global
           :boundary soft-plus
           :authority "GDPR/EU data boundary inside global"
           :pins [storage kms logs support-access customer-data])
         (operating-zone global-us
           :parent global
           :boundary default
           :authority "global US default runtime"))
	      :invariants ["Partition id MUST appear in issuer, audience, storage bucket prefix, KMS/KEK id, event topic, payment account, and router policy."
	                   "A token, API key, storage credential, model prompt route, or payment webhook from one hard partition MUST NOT be accepted by another hard partition."
	                   "Cross-partition movement requires a fresh authentication/export flow and must be classified by cross-region-data-policy."])
    (xjp-platform-partition-contract
      :fields [platform-partition legal-region runtime-target runtime-provider deploy-center-agent auth-stack secret-store payment-ledger storage-ledger router-policy eventhub timeline deploy-center-lane application-bindings status]
      :rule "XJP infrastructure, not each application, owns the cn/global separation. New data-bearing applications bind to a platform partition and inherit its auth, secret-store, payment, storage, router, event, deployment, and observability boundaries."
      :partitions
        ((xjp-cn
           :legal-region cn
           :runtime-target ecs-pcea
           :runtime-provider aliyun-ecs
           :deploy-center-agent ecs
           :auth-stack auth-cn
           :secret-store secret-store-cn
           :payment-ledger xjp-cn-ledger
           :storage-ledger xjp-cn-storage
           :router-policy xjp-cn-router
           :eventhub xjp-cn-eventhub
           :timeline xjp-cn-timeline
           :deploy-center-lane xjp-cn-deploy
           :application-bindings [pcea-cn cuthub-cn]
           :status active-cn-platform)
         (xjp-global
           :legal-region global
           :runtime-target gcp-runtime
           :runtime-provider gcp-vm
           :deploy-center-agent gcp
           :auth-stack auth-global
           :secret-store secret-store-global
           :payment-ledger xjp-global-ledger
           :storage-ledger (xjp-global-storage :provider google-cloud-storage :bucket "gs://xjp-global-object-store-project-20250408" :location ASIA :ubla true)
           :router-policy xjp-global-router
           :eventhub xjp-global-eventhub
           :timeline xjp-global-timeline
           :deploy-center-lane xjp-global-deploy
           :application-bindings [pcea-global cuthub-global]
           :status active-global-platform)
         (xjp-global-eu
           :parent xjp-global
           :legal-region eu
           :runtime-target gcp-runtime
           :runtime-provider gcp-vm
           :deploy-center-agent gcp
           :auth-stack auth-global
           :storage-ledger xjp-global-eu-storage
           :router-policy xjp-global-eu-router
           :eventhub xjp-global-eu-eventhub
           :application-bindings [pcea-global-eu]
           :status operating-zone-pending-dedicated-eu-runtime))
      :invariants ["Applications bind to exactly one hard platform partition for active user data; dual-homed user records are forbidden."
                   "An app-level partition may narrow storage/model/payment policy, but it cannot weaken the platform partition boundary."
                   "Deploy Center must expose platform-partition release provenance before MissionD can mark an app-region target deployed."])
    (regional-auth-issuer-contract
      :fields [partition issuer jwks audience oauth-clients token-signing-key session-store account-link-policy]
      :pcea ((pcea-cn :issuer "https://auth.pcea.cn" :jwks "https://auth.pcea.cn/.well-known/jwks.json" :audience pcea-cn :account-link-policy separate-account)
             (pcea-global :issuer "https://auth.pcea.io" :jwks "https://auth.pcea.io/.well-known/jwks.json" :audience pcea-global :account-link-policy separate-account)
             (pcea-global-eu :issuer "https://auth.pcea.io" :audience pcea-global-eu :session-store eu-pinned))
      :cuthub ((cuthub-cn :issuer "https://auth.cuthub.cn" :domain "cuthub.cn" :account-link-policy separate-account)
               (cuthub-global :issuer "https://auth.cuthub.com" :domain "cuthub.com" :account-link-policy separate-account))
      :forbidden [cross-partition-token-trust parent-domain-cookie-sharing shared-jwks-between-cn-and-global])
    (regional-secret-store-contract
      :fields [partition secret-store-url secret_ref_namespace kek_id kms_provider rotation_policy break_glass_policy]
      :rule "Secret values live in secret-store only. Lisp records namespaced secret refs and region/KMS ownership; it never carries values."
      :pcea ((pcea-cn :secret-namespace "pcea/cn" :kek "pcea-cn-kek" :kms-provider mainland-kms)
             (pcea-global-us :secret-namespace "pcea/global/us" :kek "pcea-global-us-kek" :kms-provider aws-kms-us)
             (pcea-global-eu :secret-namespace "pcea/global/eu" :kek "pcea-global-eu-kek" :kms-provider aws-kms-eu)))
    (regional-runtime-target-contract
      :fields [partition runtime-target runtime-provider deploy-center-agent public-domain deploy-mode artifact-flow release-provenance smoke rollback]
      :rule "Data partitions must bind to explicit deploy-center runtime targets before production rollout. MissionD records the intended placement and refuses to infer region placement from IP, domain, git branch, or a stale deploy row."
      :pcea ((pcea-cn :runtime-target ecs-pcea :runtime-provider aliyun-ecs :deploy-center-agent ecs :public-domain "pcea.top" :deploy-mode current-production-cn-compatible :artifact-flow [github-actions privatecloud-runner oss-cn-shanghai deploy-center ecs-agent] :must-not-build-on-target true)
             (pcea-global :runtime-target gcp-runtime :runtime-provider gcp-vm :deploy-center-agent gcp :public-domain "pcea.io" :deploy-mode target-pending-provisioning :requires [deploy-center-project secret-store-namespace storage-ledger payment-ledger auth-issuer smoke-rollout])
             (pcea-global-eu :runtime-target gcp-runtime :runtime-provider gcp-vm :deploy-center-agent gcp :deploy-mode operating-zone-pending-dedicated-eu-runtime :requires [eu-storage-kms-support-access-pinning]))
      :invariants ["CN PCEA production traffic targets Aliyun ECS until an explicit deploy-center migration task proves otherwise."
                   "Global PCEA traffic targets the GCP VM lane, but it MUST use a separate deploy-center project/release provenance from the CN/ECS lane."
                   "A global rollout may reuse source code and container templates, but MUST NOT reuse CN secrets, ledgers, object stores, vector stores, or user tables."])
    (regional-storage-contract
      :fields [partition object-store vector-store database log-store backup-store encryption-key data-classification]
      :rule "User files, subtitles, RAG chunks, embeddings, prompts, logs, and backups are region-pinned. Code, templates, and compiled artifacts are not user data and may be mirrored globally."
      :pcea ((pcea-cn :object-store oss-cn :vector-store milvus-cn :database postgres-cn :log-store log-cn)
             (pcea-global-us :object-store gcs-global-asia :bucket "gs://xjp-global-object-store-project-20250408" :vector-store pgvector-us :database postgres-us :log-store log-us)
             (pcea-global-eu :object-store gcs-global-eu-pending :bucket "gs://xjp-global-object-store-project-20250408/eu-pending" :vector-store pgvector-eu :database postgres-eu :log-store log-eu)))
    (regional-payment-ledger-contract
      :fields [partition legal-entity payment-provider currency ledger-db tax-invoice-policy allowed-aggregate-egress]
      :rule "Payment ledgers are hard-partitioned because acquiring, settlement, tax, invoice, and AML obligations differ by region. Cross-region finance egress is monthly aggregate only, never transaction detail."
      :pcea ((pcea-cn :provider [wechat-pay alipay mainland-psp] :currency CNY :invoice fapiao :ledger pcea-cn-ledger)
             (pcea-global-us :provider stripe-us :currency USD :ledger pcea-global-us-ledger)
             (pcea-global-eu :provider stripe-eu :currency EUR :ledger pcea-global-eu-ledger)))
    (regional-router-model-policy
      :fields [partition allowed_models denied_models pii_prompt_policy embedding_region rerank_region rag_corpus_region model_audit_log]
      :rule "Prompt and embedding data inherit the region of the project/workspace. Mainland partitions use mainland-approved models. EU user data uses EU-pinned providers or zero-data-retention agreements. DeepSeek/Qwen/Doubao are denied for global PI unless a project-specific privacy review allows a non-PI path."
      :pcea ((pcea-cn :allowed_models [qwen doubao ernie kimi] :denied_models [openai-public anthropic-public deepseek-for-pi] :pii_prompt_policy cn-only)
             (pcea-global-us :allowed_models [openai-us anthropic-bedrock-us gemini-vertex-us] :denied_models [qwen-for-pi doubao-for-pi deepseek-for-pi] :pii_prompt_policy us-global)
             (pcea-global-eu :allowed_models [openai-eu anthropic-bedrock-eu gemini-vertex-eu] :denied_models [qwen-for-pi doubao-for-pi deepseek-for-pi openai-public-us-for-eu-pi] :pii_prompt_policy eu-pinned)))
    (cross-region-data-policy
      :default deny
      :allowed-egress-categories
        ((category anonymized-aggregate-metrics :requires [k-anonymity>=50 no-user-id no-prompt-content no-transaction-detail dpo-review])
         (category public-content :requires [no-pii marketing-or-docs-review])
         (category security-threat-intelligence :requires [secops-approval encrypted audited])
         (category compliance-approved-export :requires [legal dpo business-owner export-record data-fingerprint])
         (category code-and-artifacts :requires [no-pii no-config no-secret]))
      :audit (:log central-audit-log :retention "5 years" :cadence quarterly-dpo-review))
	    (project-region-declaration :project pcea
	      :status active-ssot-required
	      :data-regions [cn global]
	      :primary-region global
      :operating-zones [global-us global-eu]
      :contains-personal-data true
      :contains-spi true
	      :contains-important-data unknown
	      :contains-children-data false
	      :cross-region-default deny
	      :platform-partition-binding ((pcea-cn :platform xjp-cn :service-stack [auth-cn secret-store-cn xjp-cn-router xjp-cn-eventhub xjp-cn-ledger])
	                                   (pcea-global :platform xjp-global :service-stack [auth-global secret-store-global xjp-global-router xjp-global-eventhub xjp-global-ledger])
	                                   (pcea-global-eu :platform xjp-global-eu :service-stack [auth-global secret-store-global xjp-global-eu-router xjp-global-eu-eventhub xjp-global-ledger]))
	      :runtime-placement ((pcea-cn :runtime-target ecs-pcea :runtime-provider aliyun-ecs :deploy-center-agent ecs :status current-production-cn-compatible)
	                          (pcea-global :runtime-target gcp-runtime :runtime-provider gcp-vm :deploy-center-agent gcp :status target-pending-provisioning)
	                          (pcea-global-eu :runtime-target gcp-runtime :runtime-provider gcp-vm :deploy-center-agent gcp :status operating-zone-pending-dedicated-eu-runtime))
      :partition-primary-key project_id
      :shared-assets [source-code lisp-blueprints ocaml-compiled-ir rust-binaries container-image-templates anonymized-aggregate-metrics]
      :forbidden [single-instance-multi-region cross-partition-token-trust shared-payment-ledger parent-domain-cookie-sharing ip-only-region-binding]
      :launch-blockers [cn-legal-entity icp-b25 mainland-psp cn-ai-filing important-data-assessment]
      :checker ["node scripts/check-v3-data-residency-universe-isomorphism.mjs" "bash /Users/jinchen/Downloads/PCEA\\ develop/.missiond/check.sh"])
    (project-region-declaration :project cuthub
      :status design-required
	      :data-regions [cn global]
	      :domains ((cn "cuthub.cn") (global "cuthub.com"))
	      :platform-partition-binding ((cuthub-cn :platform xjp-cn :service-stack [auth-cn secret-store-cn xjp-cn-router xjp-cn-eventhub xjp-cn-ledger])
	                                   (cuthub-global :platform xjp-global :service-stack [auth-global secret-store-global xjp-global-router xjp-global-eventhub xjp-global-ledger]))
	      :account-region-binding [explicit-choice phone-country-code payment-method]
      :ip-policy hint-only
      :forbidden [parent-domain-cookie-sharing online-region-switch cn-dot-global-subdomain single-account-dual-skin]
      :next-action "Promote CUTHUB to M6 only after its local SSOT declares the same partition model and checker pins.")
    :surfaces [".missiond/v3/missiond-blueprint.lisp"
               ".missiond/research/data-residency-universe-report-20260512.md"
               "scripts/check-v3-data-residency-universe-isomorphism.mjs"
               "/Users/jinchen/Downloads/PCEA develop/.missiond/intent.lisp"
               "/Users/jinchen/Downloads/PCEA develop/.missiond/check.sh"])

  (deploy-agent-self-update-governance
    :schema "missiond.deploy-agent-self-update-governance.v1"
    :owner deploy-center
    :authority-table deploy_agent_update_provenance
    :facts [agent_id current_version desired_version s3_latest update_status canary_status rollback_marker last_error]
    :events [agent_update_started agent_update_succeeded agent_update_failed rollback_started rollback_succeeded rollback_failed agent_heartbeat agent_offline]
    :rule "deploy-agent self-update and reachability status are deploy-center runtime facts stored in deploy_agent_update_provenance and heartbeat/provenance tables; deploy-center relays update/offline events into MissionD EventBridge so deploy-ops BoardTasks can be triggered from durable events. A failed best-effort notify must not be hidden inside a globally successful release summary: per-agent failure remains actionable until the target reports the desired version or an approved break-glass runbook closes the incident.")

  (project-maturity-model
    :schema "missiond.project-maturity-model.v2"
    :rule "M6 is the highest maturity level and means Auth-grade production-ready SSOT/code/runtime/test clarity: domain model, policy, flow, event, runtime projection, implementation map, compatibility ledger, hot-path wiring, regression matrix, source hygiene, and data-residency declarations for data-bearing projects are fine-grained, code-aligned, and formatter-converged."
    :gate "scripts/check-project-maturity.mjs --min-level M5 is the default universe operational gate; scripts/check-project-maturity.mjs --min-level M6 proves Auth-grade final maturity."
    :levels
      ((level M0 :name raw :requires [] :meaning "unregistered or only scattered facts")
       (level M1 :name registered-intent :requires [project-registration intent-l1-index])
       (level M2 :name blueprint-split :requires [M1 project-blueprint pillar-function-entry-core-egress-surface ordered-steps])
       (level M3 :name code-mapped :requires [M2 code-isomorphism-checker current-code-mapping drift-policy])
       (level M4 :name runtime-projected :requires [M3 runtime-config-from-lisp event-contract deploy-runtime-constants no-hardcoded-runtime-duplicates])
       (level M5 :name worker-operational :requires [M4 mission_swarm_run context-pack-shards scoped-write-guards durable-completion-evidence final-convergence-gate])
       (level M6 :name auth-grade :requires [M5 domain-model policy flow event runtime-projection implementation-map compatibility-ledger hot-path-wiring regression-matrix data-residency-declaration formatter-converged final-m6-report] :meaning "Auth-grade: the project is fine-grained, clear, runtime-wired, region-aware where it carries regulated data, regression-proven, formatter-safe, and safe for long-term dependency."))
    :invariants
      ["Project SSOT reports MUST use only M0..M6; H-levels and M10 are retired public maturity vocabulary."
       "Old M10 maps to new M5 unless the project also has Auth-grade depth evidence."
       "M6 requires Auth-grade domain/policy/flow/event/runtime/implementation/compatibility/hot-path/regression evidence plus formatter convergence: official project formatter checks must be safe to run without unrelated churn. Data-bearing projects also require a data-residency declaration that states region partitions, cross-region defaults, data classes, and compliance blockers."
       "Universe status MUST expose current and target maturity for each registered project."
       "Intent-only projects MUST NOT be marked M2+; projects without code-isomorphism evidence MUST NOT be marked M3+."
       "Resident master and swarm runners MUST use M6 SSOT convergence language and never create H-level tasks."])

  (project-maturity-registry
    :schema "missiond.project-maturity-registry.v2"
    :default-target M6
    :common-m5-to-m6-gap [domain-model policy-flow-event-split compatibility-ledger hot-path-wiring regression-matrix data-residency-declaration final-m6-report]
    (maturity :id missiond :current M6 :target M6 :gap [])
    (maturity :id board :current M5 :target M6 :gap [frontend-domain-model cockpit-hot-path-regressions final-m6-report])
    (maturity :id jarvis :current M5 :target M6 :gap [domain-shard-split missiond-integration-boundary final-m6-report])
    (maturity :id jarvis-forge :current M6 :target M6 :gap [])
    (maturity :id jarvis-mechanic :current M5 :target M6 :gap [mechanic-workflow-boundary missiond-overlap-ledger final-m6-report])
    (maturity :id xjpcode :current M5 :target M6 :gap [domain-shard-split codegen-policy-ledger final-m6-report])
    (maturity :id neural-codegen :current M5 :target M6 :gap [domain-shard-split generation-policy-hot-path final-m6-report])
    (maturity :id semantic-terminal :current M5 :target M6 :gap [domain-shard-split terminal-event-contract final-m6-report])
    (maturity :id xiaojinpro-backend :current M5 :target M6 :gap [monorepo-service-boundary deploy-fact-authority final-m6-report])
    (maturity :id deploy-center :current M6 :target M6 :gap [])
    (maturity :id xjp-memory :current M6 :target M6 :gap [])
    (maturity :id xjp-eventhub :current M6 :target M6 :gap [])
    (maturity :id xjp-mcp :current M5 :target M6 :gap [tool-policy-ledger mcp-permission-regressions final-m6-report])
    (maturity :id xjp-cli :current M5 :target M6 :gap [command-policy-ledger mcp-parity-regressions final-m6-report])
    (maturity :id deploy-agent :current M6 :target M6 :gap [])
    (maturity :id auth :current M6 :target M6 :gap [])
    (maturity :id router :current M6 :target M6 :gap [])
    (maturity :id payments :current M6 :target M6 :gap [])
    (maturity :id asr :current M5 :target M6 :gap [job-provider-transcript-domain callback-regressions final-m6-report])
    (maturity :id timeline :current M5 :target M6 :gap [revision-event-authority service-event-regressions final-m6-report])
    (maturity :id pcea :current M6 :target M6 :gap [])
    (maturity :id xiaojinpro-ios :current M6 :target M6 :gap [])
    (maturity :id secret-store :current M5 :target M6 :gap [secret-version-rotation-domain capability-regressions final-m6-report])
    (maturity :id xiaojin-blog :current M5 :target M6 :gap [content-publishing-domain deploy-auth-boundary final-m6-report])
    (maturity :id cuthub :current M5 :target M6 :gap [community-domain auth-product-dependency final-m6-report])
    (maturity :id legacy-refactor-service :current M5 :target M6 :gap [deep-code-rewrite-worker customer-frontend forge-runtime-provider production-deploy-provenance final-m6-report]))

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
      :checks ["bash .missiond/check.sh"]
      :missiond-role "registered project; Lisp/component reuse engine, not MissionD runtime orchestrator"
      :surface project-registry)
    ;; ── Part1 devtools — sibling devtool repos with M5 SSOT, registered as a group ──
    (project :id jarvis
      :kind rust-multi-crate
      :root "/Users/jinchen/Projects/jarvis"
      :intent ".missiond/intent.lisp"
      :backend ".missiond/backend/jarvis-backend-blueprint.lisp"
      :frontend ".missiond/frontend/jarvis-ui-blueprint.lisp"
      :status project-ssot-owned
      :checks ["bash .missiond/check.sh"]
      :missiond-role "registered devtool; clean MissionD rewrite (intent.lisp + 14 intent-*.lisp shards + GAP_ANALYSIS.md)"
      :surface project-registry)
    (project :id jarvis-mechanic
      :kind rust-cli
      :root "/Users/jinchen/Projects/jarvis-mechanic"
      :intent ".missiond/intent.lisp"
      :backend ".missiond/backend/jarvis-mechanic-backend-blueprint.lisp"
      :status project-ssot-owned
      :checks ["node scripts/check-mechanic-ssot.mjs"
               "bash .missiond/check.sh"]
      :missiond-role "registered devtool; opt-in repair executor CLI, not a MissionD orchestrator or automatic runtime worker"
      :surface project-registry)
    (project :id xjpcode
      :kind rust-cli
      :root "/Users/jinchen/Projects/xjpcode"
      :intent ".missiond/intent.lisp"
      :backend ".missiond/xjpcode-app-blueprint.lisp"
      :status project-ssot-owned
      :checks ["node scripts/check-xjpcode-ssot-complete.mjs --json"
               "node scripts/check-xjpcode-code-isomorphism.mjs"]
      :missiond-role "registered devtool; ratatui TUI Rust CLI agent"
      :surface project-registry)
    (project :id neural-codegen
      :kind rust-multi-crate
      :root "/Users/jinchen/Projects/neural-codegen"
      :intent ".missiond/intent.lisp"
      :backend ".missiond/neural-codegen-blueprint.lisp"
      :status project-ssot-owned
      :checks ["bash .missiond/check.sh"]
      :missiond-role "registered devtool; deterministic Lisp→IR→Rust codegen pipeline"
      :surface project-registry)
    (project :id semantic-terminal
      :kind rust-napi-cdylib
      :root "/Users/jinchen/Projects/semantic-terminal"
      :intent ".missiond/intent.lisp"
      :backend ".missiond/semantic-terminal-blueprint.lisp"
      :status project-ssot-owned
      :checks ["bash .missiond/check.sh"]
      :missiond-role "registered devtool; PTY semantic event parser (Rust core + N-API)"
      :surface project-registry)
    (project :id xiaojinpro-backend
      :kind rust-monorepo
      :root "/Users/jinchen/Downloads/xiaojinpro-gateway/xiaojinpro-backend"
      :intent ".missiond/intent.lisp"
      :backend ".missiond/backend/xiaojinpro-backend-blueprint.lisp"
      :status v3-runtime-ssot
      :checks ["node scripts/check-xjp-ssot-complete.mjs"]
      :surface project-registry)
    (project :id xjp-mcp
      :kind node-mcp-server
      :root "/Users/jinchen/Downloads/xiaojinpro-gateway/xiaojinpro-backend/tools/xjp-mcp"
      :intent ".missiond/intent.lisp"
      :backend ".missiond/backend/xjp-mcp-backend-blueprint.lisp"
      :status project-ssot-owned
      :checks ["bash .missiond/check.sh"]
      :missiond-role "registered XJP infra tool surface; ClaudeCode/MissionD-facing MCP bridge for deploy/auth/secret/storage/router operations, not deployment fact authority"
      :surface project-registry)
    (project :id xjp-cli
      :kind rust-cli
      :root "/Users/jinchen/Downloads/xiaojinpro-gateway/xiaojinpro-backend/crates/xjp-cli"
      :intent ".missiond/intent.lisp"
      :backend ".missiond/backend/xjp-cli-backend-blueprint.lisp"
      :status project-ssot-owned
      :checks ["bash .missiond/check.sh"]
      :missiond-role "registered XJP infra operator CLI and embedded MCP server; distinct from apps/xjp-deploy-agent remote execution daemon"
      :surface project-registry)
    (project :id deploy-center
      :aliases [xjp-deploy-center]
      :kind ops-service
      :root "/Users/jinchen/Downloads/xiaojinpro-gateway/xiaojinpro-backend/services/deploy-center"
      :intent ".missiond/intent.lisp"
      :backend ".missiond/backend/deploy-center-backend-blueprint.lisp"
      :status v3-runtime-ssot
      :capability deploy-ops
      :note "xjp-deploy-center is a historical alias for this same canonical service root, not an active Universe project."
      :surface project-registry)
    (project :id deploy-agent
      :aliases [xjp-deploy-agent]
      :kind ops-agent
      :root "/Users/jinchen/Downloads/xiaojinpro-gateway/xiaojinpro-backend/apps/xjp-deploy-agent"
      :intent ".missiond/intent.lisp"
      :backend ".missiond/backend/deploy-agent-backend-blueprint.lisp"
      :status project-ssot-owned
      :capability deploy-ops
      :surface project-registry)
    (project :id auth
      :kind rust-service
      :root "/Users/jinchen/Downloads/xiaojinpro-gateway/xiaojinpro-backend/services/auth"
      :intent ".missiond/intent.lisp"
      :backend ".missiond/backend/auth-backend-blueprint.lisp"
      :status v3-runtime-ssot
      :surface project-registry)
    (project :id router
      :kind rust-service
      :root "/Users/jinchen/Downloads/xiaojinpro-gateway/xiaojinpro-backend/services/router"
      :intent ".missiond/intent.lisp"
      :backend ".missiond/backend/router-backend-blueprint.lisp"
      :status v3-runtime-ssot
      :surface project-registry)
    (project :id xjp-memory
      :kind rust-service
      :root "/Users/jinchen/Downloads/xiaojinpro-gateway/xiaojinpro-backend/services/xjp-memory"
      :intent ".missiond/intent.lisp"
      :backend ".missiond/backend/xjp-memory-backend-blueprint.lisp"
      :status contract-first-service
      :checks ["bash .missiond/check.sh"]
      :missiond-role "registered memory provider service; owns private memory, review overlay, skill evidence, FTS/embedding/rerank storage behind MissionD memory-provider-contract"
      :surface project-registry)
    (project :id xjp-eventhub
      :kind rust-service
      :root "/Users/jinchen/Downloads/xiaojinpro-gateway/xiaojinpro-backend/services/xjp-eventhub"
      :intent ".missiond/intent.lisp"
      :backend ".missiond/backend/xjp-eventhub-backend-blueprint.lisp"
      :status contract-first-service
      :checks ["bash .missiond/check.sh"]
      :missiond-role "registered EventHub service; owns cross-service durable event envelopes while MissionD local EventBus remains offline-safe"
      :surface project-registry)
    (project :id payments
      :kind rust-workspace-service
      :root "/Users/jinchen/Downloads/xiaojinpro-gateway/xiaojinpro-backend/services/payments"
      :intent ".missiond/intent.lisp"
      :backend ".missiond/backend/payments-backend-blueprint.lisp"
      :status v3-runtime-ssot
      :surface project-registry)
    (project :id asr
      :kind rust-service
      :root "/Users/jinchen/Downloads/xiaojinpro-gateway/xiaojinpro-backend/services/asr"
      :intent ".missiond/intent.lisp"
      :backend ".missiond/backend/asr-backend-blueprint.lisp"
      :status v3-runtime-ssot
      :surface project-registry)
    (project :id timeline
      :kind rust-service
      :root "/Users/jinchen/Downloads/xiaojinpro-gateway/xiaojinpro-backend/services/timeline"
      :intent ".missiond/intent.lisp"
      :backend ".missiond/backend/timeline-backend-blueprint.lisp"
      :status v3-runtime-ssot
      :surface project-registry)
    (project :id pcea
      :kind rust-vite-app
      :root "/Users/jinchen/Downloads/PCEA develop"
      :intent ".missiond/intent.lisp"
      :backend ".missiond/backend/pcea-backend-blueprint.lisp"
      :frontend ".missiond/frontend/pcea-frontend-blueprint.lisp"
      :status project-ssot-owned
      :surface project-registry)
    (project :id xiaojinpro-ios
      :kind ios-swiftui-app
      :root "/Users/jinchen/development/xiaojinproIOS/xiaojinpro"
      :intent ".missiond/intent.lisp"
      :frontend ".missiond/frontend/xiaojinpro-ios-blueprint.lisp"
      :operations ".missiond/operations/xiaojinpro-ios-operations-blueprint.lisp"
      :status project-ssot-owned
      :checks ["bash .missiond/check.sh"]
      :missiond-role "registered mobile control client; iPhone entry for Jarvis/MissionD, using Auth JWT and Jarvis HTTPS proxy to control the Mac mini MissionD node"
      :surface project-registry)
    ;; ── App + external-infra projects — already-converged with project-local check.sh runners ──
    (project :id secret-store
      :aliases [secret-store-rs]
      :kind rust-axum-microservice
      :root "/Users/jinchen/Downloads/xiaojinpro-gateway/services/secret-store-rs"
      :intent ".missiond/intent.lisp"
      :status project-ssot-owned
      :lifecycle external-infra-runtime
      :checks ["bash .missiond/check.sh"]
      :missiond-role "registered external infra runtime; AES-256-GCM credential vault (frozen LTS) consumed by auth/deploy-center/* via xjp-config HybridSecretProvider; production endpoint ss.xiaojinpro.top is now on the GCP xjp-backend VM with Caddy proxy to the local secret-store container"
      :surface project-registry)
    (project :id xiaojin-blog
      :kind nextjs-app
      :root "/Users/jinchen/Projects/xiaojin-blog"
      :intent ".missiond/intent.lisp"
      :status project-ssot-owned
      :checks ["bash .missiond/check.sh"]
      :missiond-role "registered app; ruoqijin.com personal blog + research portal (Next.js 16 + React 19 + Drizzle/PG; standalone repo xiaojinpro-team/xiaojin-blog)"
      :surface project-registry)
    (project :id cuthub
      :kind nextjs-app
      :root "/Users/jinchen/Downloads/xiaojinpro-gateway/xiaojinpro-backend/cuthub-frontend"
      :intent ".missiond/intent.lisp"
      :status project-ssot-owned
      :lifecycle canonical-temporary-downloads-checkout
      :note "Supervisor decision 39a2e6e8 — Downloads checkout accepted as temporary canonical M6 SSOT root until repo is cloned to /Users/jinchen/Projects/cuthub-frontend"
      :checks ["bash .missiond/check.sh"]
      :missiond-role "registered app; cuthub.ai frontend (Next.js 16 + React 19 + Tailwind 4 + Konva); independent repo rickyjim626/cuthub-frontend"
      :surface project-registry)
    (project :id legacy-refactor-service
      :kind node-service
      :root "/Users/jinchen/Projects/legacy-refactor-service"
      :intent ".missiond/intent.lisp"
      :backend ".missiond/backend/legacy-refactor-backend-blueprint.lisp"
      :operations ".missiond/deploy/legacy-refactor-deploy-blueprint.lisp"
      :status external-product-service
      :checks ["node scripts/check-legacy-refactor-ssot.mjs --json"]
      :missiond-role "registered external product service; MissionD may orchestrate and observe jobs, while the service owns customer-safe refactor runtime and never exposes internal Lisp/IR/Forge artifacts to customers"
      :surface project-registry))

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

  (service-runtime-universe
    :schema "missiond.service-runtime-universe.v1"
    :rule "Production service runtime facts are Lisp-owned Universe data: project/service roots, domains, deployments, health, DNS capability, and ops owner are visible to resident master and workers through mission_project(action=universe). Secrets stay outside Lisp."
    (service :id auth
      :project xiaojinpro-backend
      :root "/Users/jinchen/Downloads/xiaojinpro-gateway/xiaojinpro-backend/services/auth"
      :intent ".missiond/intent.lisp"
      :backend ".missiond/backend/auth-backend-blueprint.lisp"
      :environment production
      :public-base-url "https://auth.xiaojinpro.com"
      :issuer "https://auth.xiaojinpro.com"
      :domains ["auth.xiaojinpro.com"]
      :dns-provider cloudflare
      :dns-capability (:read-inventory true :mutate requires-board-approval :secret-source env)
      :deployment (:substrate kubernetes :namespace production :deployment "xjp-auth-center" :service "xjp-auth-center" :replicas 3 :hpa-min 3 :hpa-max 10 :image "xjp-auth-center:latest" :service-account "xjp-auth-center")
      :proxy (:kind caddy :domain "auth.xiaojinpro.com" :file "/Users/jinchen/Downloads/xiaojinpro-gateway/xiaojinpro-backend/services/auth/caddy/Caddyfile" :sse-no-buffer "/auth/login-stream")
      :ports (:http 8081 :metrics 9090 :service 80)
      :health ["/health/live" "/health/ready" "/.well-known/openid-configuration" "/.well-known/jwks.json"]
      :event-ingest (:endpoint "/webhooks/auth-event" :domain system :event ExternalServiceEvent :source auth-audit-events :token-env MISSIOND_EXTERNAL_WEBHOOK_TOKEN :authority provider-durable-log-first :rule "Auth emits sanitized service events into MissionD EventBus via deploy-center adapter; X-MissionD-Webhook-Token is required when MISSIOND_EXTERNAL_WEBHOOK_TOKEN is configured; PTY is diagnostic only and MissionD must not require production probing to observe auth incidents.")
      :dependencies [postgres redis secret-store wechat-open-platform google-oauth sms-provider email-provider]
      :ops-capability deploy-ops
      :source-evidence ["/Users/jinchen/Downloads/xiaojinpro-gateway/xiaojinpro-backend/services/auth/k8s/production/configmap.yaml" "/Users/jinchen/Downloads/xiaojinpro-gateway/xiaojinpro-backend/services/auth/k8s/production/deployment.yaml" "/Users/jinchen/Downloads/xiaojinpro-gateway/xiaojinpro-backend/services/auth/caddy/Caddyfile"]
      :risks [wechat-callback-prod-drift mysql-artifact-cleanup])
    (service :id deploy-center
      :project deploy-center
      :root "/Users/jinchen/Downloads/xiaojinpro-gateway/xiaojinpro-backend/services/deploy-center"
      :intent ".missiond/intent.lisp"
      :backend ".missiond/backend/deploy-center-backend-blueprint.lisp"
      :environment production
      :deployment (:substrate deploy-center :authority release-provenance :provenance-api "/api/deploy/provenance/:project")
      :deployment-confirmation (:checker "node scripts/check-m6-deployment-status.mjs --json" :status-api "/api/deploy/status" :rollout-workflow ".missiond/workflows/m6-deployment-rollout.lisp")
      :event-ingest (:endpoint "/webhooks/deploy-center-event" :domain system :event ExternalServiceEvent :source deploy_events :token-env MISSIOND_EXTERNAL_WEBHOOK_TOKEN :authority deploy-center.deploy_events :rule "deploy-center relays durable deploy_events rows into MissionD EventBridge with stable event_id and MissionD idempotency; MissionD must not infer production release state by stitching GitHub/curl/git when deploy-center has provenance.")
      :events [deploy_created build_started build_succeeded build_failed deploy_started deploy_succeeded deploy_failed smoke_succeeded smoke_failed rollback_started rollback_succeeded rollback_failed agent_heartbeat agent_update_started agent_update_succeeded agent_update_failed provenance_changed]
      :ops-capability deploy-ops
      :surface service-runtime-universe)
    (service :id secret-store
      :project secret-store
      :root "/Users/jinchen/Downloads/xiaojinpro-gateway/services/secret-store-rs"
      :intent ".missiond/intent.lisp"
      :environment production
      :public-base-url "https://ss.xiaojinpro.top"
      :domains ["ss.xiaojinpro.top"]
      :deployment (:substrate gcp-vm :runtime-target gcp-runtime :container "secret-store" :local-bind "127.0.0.1:8091" :proxy caddy :authority deploy-center-provenance)
      :health ["/livez" "/readyz"]
      :dependencies [xjp-postgres secret-store-kek admin-key]
      :ops-capability deploy-ops
      :source-evidence [secret-store-gcp-migration-20260511]
      :surface service-runtime-universe)
    (service :id secret-store-cn
      :project secret-store
      :root "/Users/jinchen/Downloads/xiaojinpro-gateway/services/secret-store-rs"
      :intent ".missiond/intent.lisp"
      :environment cn-production
      :public-base-url "https://ss-cn.xiaojinpro.com"
      :domains ["ss-cn.xiaojinpro.com"]
      :deployment (:substrate aliyun-ecs :dc_slug "secret-store-cn" :runtime-target ecs-pcea :network-profile ecs-cn-restricted :executor ecs-agent :work_dir "/opt/secret-store-cn" :compose_file "/opt/secret-store-cn/docker-compose.cn.yml" :local-bind "127.0.0.1:8091" :proxy nginx :artifact-delivery-lane cn-oss-bundle-lane :authority verified-smoke :deploy-center-status stale-runtime-shell :provenance partial)
      :health ["/livez" "/readyz"]
      :dependencies [cn-postgres secret-store-cn-kek secret-store-cn-admin-key]
      :ops-capability deploy-ops
      :source-evidence [secret-store-cn-ecs-deploy-20260513 secret-store-cn-runtime-verified-20260515 skill:secret-store skill:aliyun]
      :risks [deploy-center-read-model-gap provenance-contract-required docker-healthcheck-disabled-until-next-image-promotion]
      :surface service-runtime-universe)
    (service :id xjp-memory
      :project xjp-memory
      :root "/Users/jinchen/Downloads/xiaojinpro-gateway/xiaojinpro-backend/services/xjp-memory"
      :intent ".missiond/intent.lisp"
      :backend ".missiond/backend/xjp-memory-backend-blueprint.lisp"
      :environment local-dev
      :deployment (:substrate deploy-center :dc_slug "xjp-memory" :container_name "xjp-memory" :default-port 8091 :authority release-provenance)
      :local-runtime (:substrate launchd :label "com.xjp.memory.provider" :url "http://127.0.0.1:8091" :database "xjp_memory" :storage postgres-durable :bringup "scripts/manage-local-providers.sh")
      :health ["/health" "/health/live" "/health/ready" "/v1/memory/provider_status"]
      :dependencies [xjp-router secret-store postgres?]
      :ops-capability memory-provider
      :surface service-runtime-universe)
    (service :id xjp-eventhub
      :project xjp-eventhub
      :root "/Users/jinchen/Downloads/xiaojinpro-gateway/xiaojinpro-backend/services/xjp-eventhub"
      :intent ".missiond/intent.lisp"
      :backend ".missiond/backend/xjp-eventhub-backend-blueprint.lisp"
      :environment local-dev
      :deployment (:substrate deploy-center :dc_slug "xjp-eventhub" :container_name "xjp-eventhub" :default-port 8092 :authority release-provenance)
      :local-runtime (:substrate launchd :label "com.xjp.eventhub.provider" :url "http://127.0.0.1:8092" :database "xjp_eventhub" :storage postgres-durable :bringup "scripts/manage-local-providers.sh")
      :health ["/health" "/health/live" "/health/ready" "/v1/eventhub/status"]
      :dependencies [deploy-center timeline? postgres?]
      :ops-capability eventhub
      :surface service-runtime-universe)
    (service :id legacy-refactor-service
      :project legacy-refactor-service
      :root "/Users/jinchen/Projects/legacy-refactor-service"
      :intent ".missiond/intent.lisp"
      :backend ".missiond/backend/legacy-refactor-backend-blueprint.lisp"
      :environment local-dev
      :public-base-url "http://127.0.0.1:8788"
      :deployment (:substrate local-node :entrypoint "node src/server.mjs" :port-env LEGACY_REFACTOR_PORT :default-port 8788)
      :health ["/health"]
      :dependencies [forge-catalog? missiond-eventbridge?]
      :ops-capability project-refactor
      :surface service-runtime-universe)
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

  (mcp-tool-governance-policy
    :desc "Primary MCP tool families for agents; old public tools remain callable compatibility leaves, but agents should select by family first."
    :schema "missiond.mcp-tool-governance.v1"
    :primary-families [mission_board mission_workflow mission_workstation mission_context mission_memory mission_universe mission_ops mission_router mission_tool_directory]
    :directory-tool mission_tool_directory
    :max-primary-families 12
    :xjp-cli-mcp-parity
      ((authority tools/xjp-mcp)
       (operator-shell xjp-cli)
       (audit-command "xjp mcp parity --json")
       (rule "XJP MCP is the latest ClaudeCode/MissionD tool authority; xjp-cli is an operator shell and must expose parity gaps rather than implying it contains every deploy/router/storage/cloudflare tool."))
    :metadata-required [tool_family primary_action tier danger_level intent_examples preferred_surface compatibility_tools]
    :agent-rule "When unsure, call mission_tool_directory(action=\"recommend\", intent=...) before selecting a lower-level MCP tool. Tool families are a selection/readability layer; compatibility tools remain stable for existing workers."
    :invariants
      ["mission_tool_directory MUST expose list/recommend/lookup/explain/deprecated actions over the primary tool-family catalog."
       "Public tools MAY remain numerous, but every high-frequency tool must map to a primary family and preferred surface."
       "Deprecated/raw tools MUST return a preferredFamily/preferredSurface hint instead of relying on operator memory."
       "MCP tool-family governance must be read-only; it guides selection and must not mutate Board, KB, projects, or runtime state."])

  (memory-provider-contract
    :schema "missiond.memory-provider.v1"
    :purpose "Make memory pluggable so MissionD Core can be open-sourced, multi-tenant, and multi-universe without carrying private conversation, KB, skill-evidence, embedding, or review-overlay data."
    :core-boundary "MissionD Core owns provider registry, scope resolution, query/write facades, context injection policy, and MCP compatibility; providers own memory data and retrieval internals."
    :scope-fields [tenant_id universe_id project_id user_id source_type source_id authority privacy_class review_state]
    :default-provider null-memory
    :providers
      ((provider null-memory
         :kind disabled
         :use-case open-source-default
         :capabilities []
         :rule "Open-source/default MissionD can run without private memory data; queries return explicit MEMORY_PROVIDER_DISABLED diagnostics.")
       (provider local-postgres-memory
         :kind local-postgres
         :use-case single-user-dev-compatible
         :capabilities [query remember review-overlay conversation-ingest skill-evidence export purge]
         :data-owner "local MissionD database compatibility tables"
         :rule "Current MissionD KB/conversation tables are a compatibility provider implementation, not the permanent MissionD Core memory model.")
       (provider xjp-memory
         :kind remote-service
         :use-case private-multi-universe
         :capabilities [query remember review-overlay conversation-ingest skill-evidence fts embedding rerank context-pack export purge]
         :runtime-env [MISSIOND_MEMORY_PROVIDER_URL MISSIOND_MEMORY_PROVIDER_TOKEN]
         :embedding-provider xjp-router
         :rerank-provider xjp-router
         :rule "Private deployments use xjp-memory for tenant/universe/project/user scoped memory, conversation history, skill evidence, embedding, rerank, and review overlay. Secrets and provider tokens stay in secret-store/env, never in Lisp."))
    :functions
      ((function memory-provider-registry
         :entry [V3-compiled-runtime env-config mission_memory.provider_status]
         :core ((step s1 :logic "load provider declarations and active provider selection from MISSIOND_MEMORY_PROVIDER_URL / MISSIOND_MEMORY_PROVIDER_MODE")
                (step s2 :logic "validate provider capabilities against requested operation")
                (step s3 :logic "call /v1/memory/provider_status for xjp-memory providers, or return explicit null/local compatibility diagnostics"))
         :egress [MemoryProviderConfig provider-status])
       (function memory-scope-resolution
         :entry [BoardTask project-registry user-request active-universe]
         :core ((step s1 :logic "resolve tenant/universe/project/user scope before every memory query or write")
                (step s2 :logic "reject unscoped global memory reads unless workflow explicitly asks for cross-universe audit")
                (step s3 :logic "attach scope fields to provider requests and task-result artifacts"))
         :egress [memory-scope provider-namespace])
       (function memory-query-contract
         :entry [mission_memory.query mission_kb_query context-pack-builder]
         :core ((step s1 :logic "apply memory-scope-resolution")
                (step s2 :logic "apply review overlay and default active-only retrieval")
                (step s3 :logic "call provider query with explicit capability and privacy class")
                (step s4 :logic "return lane-labeled evidence without injecting broad KB into prompts by default"))
         :egress [memory-query-result context-evidence-lane])
       (function memory-write-contract
         :entry [mission_memory.remember mission_kb_remember intent-memory-capture memory-review-batch-runner]
         :core ((step s1 :logic "require explicit scope and write reason")
                (step s2 :logic "route high-confidence stable intent to provider remember")
                (step s3 :logic "route uncertain or conflicting items to review overlay/candidate artifacts")
                (step s4 :logic "preserve source_refs and supersession metadata"))
         :egress [memory-record review-candidate])
       (function memory-review-overlay-contract
         :entry [mission_memory.review mission_kb_review memory-review-batch-runner]
         :core ((step s1 :logic "write non-destructive review overlay")
                (step s2 :logic "exclude superseded/historical/duplicate/stale/delete-candidate/needs-human from default retrieval")
                (step s3 :logic "keep original evidence available with include_archived=true or state_filter"))
         :egress [review-overlay-state])
       (function memory-context-injection-policy
         :entry [resident-master context-pack-builder worker-brief]
         :core ((step s1 :logic "default to no KB prefetch")
                (step s2 :logic "inject memory only when workflow declares memory scope and evidence purpose")
                (step s3 :logic "include provider/source/scope labels so agents can distinguish long-term memory from SSOT and runtime evidence"))
         :egress [context-pack-memory-lane]))
    :invariants
      ["MissionD Core MUST NOT require private memory data to boot, run Board/workstation workflows, or pass open-source checks."
       "Every memory query/write MUST resolve tenant/universe/project/user scope before calling a provider."
       "mission_kb_query and mission_kb_remember are compatibility leaves; preferred agents use mission_memory query/remember/review/provider_status."
       "Provider implementations own FTS, embedding, rerank, conversation archive, skill evidence index, active memory, archive state, export, and purge."
       "Default context-pack generation MUST NOT preload KB/history/provider logs; memory is opt-in by workflow and scope."]
    :checker "node scripts/check-v3-service-extraction-isomorphism.mjs")

  (memory-kb-policy
    :desc "Lisp-owned memory extraction budget for the memory-kb surface."
    :pending-message-limit 60
    :tool-result-preview-chars 1000
    :assistant-preview-chars 500
    :active-memory-target-ratio 0.10
    :sensitive-query-suppression [architecture:module]
    :review-states [active superseded-by-lisp superseded-by-code historical-evidence duplicate wrong-or-stale delete-candidate needs-human]
    :default-query-policy "exclude current review states superseded-by-lisp/superseded-by-code/historical-evidence/duplicate/wrong-or-stale/delete-candidate/needs-human unless include_archived or state_filter is explicit"
    :invariants
      ["mission_memory_pending MUST project batch size and preview truncation lengths from memory-kb-policy."
       "mission_memory_pending MUST cache the served realtime extraction batch for the active extraction cycle and allow bounded replay after context compaction; if replay cache is missing or exhausted it MUST return structured MEMORY_PENDING_ALREADY_SERVED rather than a successful empty result."
       "mission_memory_pending MUST classify deployment-monitor, runtime-report, worker-instruction, and provider-preamble text noise into input skip diagnostics before active memory extraction; deployment-monitor covers deploy/build/smoke/rollback/agent-update/provenance diagnostics plus deployment-event-response, xjp_build_wait, xjp_deploy_watch, and xjp_deploy_status monitor text; user utterances MUST never be filtered by these text classifiers."
       "mission_kb_query MUST suppress architecture:module details for sensitive credential/secret/SSH/token queries unless the caller explicitly scopes category/project to that architecture surface."
       "mission_kb_query MUST support excludeCategory / exclude_category for explicit category suppression, including subcategory matches such as memory excluding memory:*."
       "mission_kb_mutate(action=batch_remember) MUST accept a bounded entries array so memory review and distillation workflows do not need to spam one MCP call per KB row."
       "mission_kb_remember MUST pass through one shared dedupe gate in KbStore::kb_remember before any realtime/deep-analysis/manual pipeline can create a new active key; same source-session duplicates use a stricter low threshold and merge evidence_refs/source_sessions/superseded_by instead of overwriting them."
       "mission_kb_review MUST write a non-destructive knowledge_review_state overlay; it MUST NOT mutate or delete the original knowledge row."
       "Low-confidence semantic duplicate candidates MUST create a needs-human knowledge_review_state artifact and leave the raw row as evidence rather than deleting or silently activating it."
       "Large KB cleanup MUST calibrate with at least five manual batches before batch overlay application; target active memory is about 10%, with needs-human hidden from default retrieval."
       "mission_kb_query default retrieval MUST honor the review overlay while include_archived=true and state_filter preserve audit access to historical evidence."
       "A real MissionD project with .missiond but no memory-kb-policy MUST return V3_BLUEPRINT_CONFIG_ERROR rather than silently using embedded defaults."])

  (learning-engine-policy
    :desc "Lisp-owned autonomous learning engine cadence, pty budget, and low-utility reflection policy."
    :realtime-extraction-timeout-secs 300
    :realtime-empty-backoff-base-secs 30
    :realtime-empty-backoff-max-secs 900
    :deep-analysis-zero-output-fuse-threshold 3
    :deep-analysis-zero-output-fuse-secs 3600
    :decision-tier3-timeout-secs 300
    :habit-scan-timeout-secs 600
    :token-spend-guard-window-secs 3600
    :token-spend-guard-soft-limit 250000
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
       "Realtime extraction MUST apply exponential empty-queue backoff from learning-engine-policy after consecutive no-user-work probes, and reset the backoff as soon as a real batch is dispatched."
       "Deep analysis MUST apply a Lisp-projected zero-output saturation fuse after consecutive completed deep-analysis jobs produce no KB mutations; while fused it MUST skip dispatch and expose diagnostics."
       "Memory/learning workers MUST consult token_usage_ledger through a Lisp-projected sliding-window token-spend soft guard before dispatch; if the window crosses token-spend-guard-soft-limit, MissionD MUST pause the memory domain through ControlTree and emit diagnostics instead of spending into a provider quota cliff."
       "Realtime extraction MUST claim the extraction lane before running pending-message DB probes; pending realtime SQL MUST use EXISTS/LATERAL LIMIT or bounded materialized-candidate shapes instead of global COUNT(DISTINCT)/ROW_NUMBER scans; deep-analysis active-conversation probes MUST use bounded EXISTS/OFFSET checks instead of full message COUNT scans so repeated ticks or status refreshes cannot exhaust the Postgres pool."
       "Memory extraction pending selectors MUST filter MissionD self-referential worker slots, including slot-memory*, slot-diagnosis*, and agent-* sessions, even when historical role attribution mistakenly labeled them as user conversations."
       "Learning maintenance cadences (timeline analysis, idle exploration, habit scan, KB auto-GC, KB consolidation, KB reflection, decision harvest, co-occurrence refresh) MUST project from learning-engine-policy."
       "Timeline analysis read windows, event limits, and slow-request threshold MUST project from learning-engine-policy."
       "KB reflection low-utility threshold, minimum access count, max entries, and max_tokens MUST project from learning-engine-policy."
       "Timeline projection SQL MUST cast string-bound since/until parameters as ::timestamptz when comparing against event_log.ts so PG never raises 'operator does not exist: timestamp with time zone >= text' from Timeline Analyst, mission_timeline, or stratified queries."
       "Timeline Analyst MUST check the Gemini provider gate before collecting timeline evidence or calling Gemini; when the gate is closed it MUST advance the cadence marker and skip without warning spam or repeated LLM attempts."])

  (conversation-ingestion-policy
    :desc "Lisp-owned read-model window and limit defaults for conversation, event, and timeline query surfaces."
    :conversation-get-tail-default 50
    :conversation-search-default-limit 10
    :message-search-default-limit 20
    :analysis-context-max-turns 50
    :label-calibration-sample-limit 200
    :jarvis-stream-envelope-schema "missiond.jarvis-stream-envelope.v1"
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
       "mission_timeline(action=wait) MUST expose bounded EventBus waits for board/slot/task/system predicates; timeout/lag returns diagnostic JSON so polling remains only an explicit fallback."
       "Explicit opt-in UserPromptSubmit context prefetch intent router model and timeout MUST project from conversation-ingestion-policy instead of local claude-opus/10000ms literals; default workstation hook sync removes UserPromptSubmit prefetch until a memory-audit workflow enables it."
       "Codex vision worker binary/model/idle timeout and CodexCli absolute timeout MUST project from conversation-ingestion-policy instead of local gpt-5.4/120s/300s literals."
       "Historical conversation event/tool-call backfills MUST NOT run unconditionally on daemon startup; they are opt-in maintenance/workflow operations gated by llm.yaml backfill_enabled or MISSIOND_CONVERSATION_BACKFILL_ON_STARTUP=1 so daemon restarts do not replay large provider histories as foreground CPU load."
       "llm_summary/topic embedding generation MUST default to human/Jarvis/direct CLI chat read models only; worker/meta/memory-slot conversations project their canonical result through task-result-artifact so skill-injection prompts, quota diagnostics, and worker instructions do not pollute user-facing conversation summaries."
       "Conversation analysis_context MUST be a bounded read model: it samples at most analysis-context-max-turns from calibrated turns and never pulls raw worker/provider chatter into user-intent inference."
       "Conversation label calibration MUST remain overlay-first: message_labels stores speaker/origin/canonical_state evidence, rawRole is preserved, and calibration reports are reviewed before any destructive rewrite."
       "Jarvis SSE and OpenAI-compatible chat surfaces MUST emit jarvis-stream-envelope-schema frames with conversation_id/task_id correlation, process affinity, and semantic event kind; PTY status is diagnostic and cannot replace the envelope."
       "Jarvis mobile/public clients MUST call /api/readiness after /health so daemon liveness, default slot busy state, and slot-unavailable startup failures are distinct operator-visible states; /health alone MUST NOT be presented as end-to-end readiness."
       "Jarvis mobile/public clients and operators MUST have /api/monitor/jarvis as the chain monitor for proxy reachability, daemon release, default slot readiness, MCP config, PTY live-screen/log evidence, and compiled runtime config; readiness is UX state, monitor is debug evidence."
       "Jarvis chat surfaces MUST write provider usage into token_usage_ledger with slot/task/message linkage so billing and quota views read one source of truth."
       "A real MissionD project with .missiond but no conversation-ingestion-policy MUST return V3_BLUEPRINT_CONFIG_ERROR rather than silently using embedded defaults."])

  (evidence-governance-policy
    :desc "Unified evidence model for Memory/KB, Logs, Timeline, Conversation, and worker outputs."
    :authority-order [task_result_artifacts provider_durable_conversation event_log knowledge_review_state board_projection]
    :roles
      ((task-result-artifact :role canonical-worker-output :rule "Worker and workflow finals land here first; Board notes are projections.")
       (conversation :role provider-user-turn-read-model :rule "Conversation rows/messages preserve provider/user turns for audit and retrieval, not completion authority.")
       (timeline :role event-causality-view :rule "event_log / EventBus projections explain when and why something happened.")
       (kb-memory :role reviewed-long-term-knowledge :rule "KB is curated active memory after review overlay; raw historical logs are not active knowledge.")
       (board :role coordination-projection :rule "BoardTask state coordinates work and operator decisions; it is not the canonical worker result body."))
    :runtime-projection [mission_shared_memory.evidence_view task_result_artifacts conversations event_log knowledge_review_state board_tasks]
    :invariants
      ["mission_shared_memory(action=evidence_view) MUST return the unified evidence governance view for a task/project, grouping task_result_artifacts, conversations, event_log/shared_events, KB review overlay, and Board projection into named evidence lanes."
       "Memory/KB, Logs, Timeline, and Conversation MUST NOT each invent their own final-result authority; worker outputs use task-result-artifact, conversations are read models, timeline is causality, KB is reviewed long-term knowledge, and Board is coordination projection."
       "Default agent context may cite the evidence view lanes, but must not treat raw PTY, raw provider transcript, or unreviewed KB as higher authority than task-result-artifacts and durable events."])

  (cli-conversation-ingestion
    :desc "Canonical CLI conversation-log ingestion contract for ClaudeCode, Gemini CLI, and Codex CLI."
    :legacy-aliases ["claude_cli" "pty_jsonl"]
    (source claude-code
      :canonical "claude_code"
      :paths ["~/.claude/projects/**/*.jsonl" "~/.claude/history.jsonl"]
      :watcher "crates/missiond-core/src/cc_tasks/watcher.rs"
      :route "crates/missiond-daemon/src/infra/ingestion_router.rs"
      :history-import "scripts/import-claude-history-jsonl.mjs"
      :normalizer "scripts/normalize-claudecode-conversations.mjs"
      :audit "scripts/audit-claudecode-conversations.mjs")
    (source gemini-cli
      :canonical "gemini_cli"
      :paths ["~/.gemini/tmp/*/chats/*.json" "~/.gemini/tmp/*/chats/*.jsonl"]
      :watcher "crates/missiond-core/src/gemini_cli/watcher.rs"
      :route "crates/missiond-daemon/src/workers/local/gemini_reconcile_worker.rs"
      :audit "scripts/audit-gemini-conversations.mjs")
    (source codex-cli
      :canonical "codex_cli"
      :paths ["~/.codex/state_5.sqlite" "~/.codex/sessions/**/*.jsonl" "~/.codex/archived_sessions/*.jsonl" "~/.codex/session_index.jsonl" "~/.codex/history.jsonl"]
      :worker "crates/missiond-daemon/src/workers/local/codex_ingestion_worker.rs")
    :invariants
      ["Conversation sources MUST be canonicalized before DB write: claude_code, gemini_cli, or codex_cli."
       "Legacy claude_cli and PTY transport pty_jsonl remain read aliases only; new non-transport source fields MUST name the canonical CLI."
       "mission_pty_status and mission_slots observability MUST be joinable with the latest conversation row by slot/session id and source."
       "mission_slots MUST reject or flag slot_sessions whose conversation source disagrees with the slot engine; stale provider drift must never masquerade as current state."
       "Codex CLI slot_sessions may contain a PTY placeholder id; mission_slots MUST fall back to the latest real codex_cli conversation for the slot project instead of surfacing a messageCount=0 placeholder as the latest durable conversation."
       "Codex CLI ingestion MUST scan the full state_5.sqlite thread set, including archived threads, and mark archived threads as historical status instead of dropping them from MissionD history."
       "Codex CLI ingestion MUST also discover raw rollout JSONL under ~/.codex/sessions/**/*.jsonl and ~/.codex/archived_sessions/*.jsonl even when state_5.sqlite has no thread row; session_meta.payload.id is the canonical conversation id, and raw-only imported rows MUST be recorded in conversation_source_state as sqlite-missing instead of being silently ignored."
       "Codex CLI conversation_source_state MUST distinguish current, sqlite-missing, missing-stale, path-mismatch, archived, and pty-placeholder evidence so audits can explain whether MissionD is missing provider history, whether Codex sqlite lost a rollout row, or whether a visible PTY is only a diagnostic placeholder."
       "Codex CLI runtime status MUST NOT treat archived=false in state_5.sqlite as proof that a conversation is actively running; provider source archive state, slot binding, durable final, and PTY state are separate evidence lanes."
       "Gemini request-log persistence MUST only consume Gemini provider LlmEvent variants; Codex CLI durable history belongs to codex_cli conversations/source-state, and non-Gemini LlmEvent replay MUST NOT pollute gemini_requests or generate duplicate insert warnings."
       "Codex CLI message ingestion MUST generate deterministic non-null message_uuid values from thread id, JSONL line number, role, and source event hash so reconcile/backfill cannot repeatedly insert duplicate NULL-uuid rows."
       "Codex CLI background ingestion MUST persist rollout size/mtime/line/complete watermarks and parse large rollout files in bounded pages after the last durable cursor; a 50k safety limit is per poll page, never a permanent history truncation."
       "When deterministic UUID ingestion meets an older NULL-uuid row with the same session, role, timestamp, and content, the DB layer MUST adopt that existing row by setting message_uuid instead of inserting a new duplicate row."
       "mission_conversation_get MUST defensively coalesce duplicate rows by message_uuid or role/timestamp/content fallback so frontend logs stay readable until historical cleanup is reviewed."
       "mission_conversation_get MUST retrieve tail messages with the indexed (session_id,id) path and assign display seq after duplicate coalescing; it MUST NOT use a ROW_NUMBER window over an entire large Codex/Gemini session."
       "Historical duplicate cleanup is dry-run/report-first; destructive DB cleanup must keep the earliest row in each duplicate group and require an explicit reviewed apply path."
       "Gemini background reconcile MUST use size/mtime companion watermarks to skip already-reconciled old chat files without reparsing full historical transcripts; manual reconcile may force a full scan."
       "Gemini manual/full reconcile MUST ignore count watermarks and replay raw ~/.gemini/tmp/*/chats/session-* files from message index 0 through deterministic message_uuid upserts, so historical sessions anchored before MissionD watcher startup can still be imported without duplicates."
       "Gemini manual/full reconcile MUST be reachable through mission_conversation_query(action=gemini_reconcile) / mission_conversation_gemini_reconcile, and that action MUST call gemini_reconcile_worker::run_gemini_reconciliation_now instead of relying on daemon restart or ad hoc SQL repair."
       "Gemini CLI tool lifecycle MUST close conversation_tool_calls with tool_result messages: parser emits tool_use/tool_result blocks, realtime ingestion and gemini_reconcile both persist has_tool_use/has_tool_result/content_types, and role=tool_result updates output_summary/raw_output/status rather than leaving tool calls pending."
       "Gemini CLI raw-vs-DB coverage MUST be auditable through scripts/audit-gemini-conversations.mjs: raw sessions missing in DB, DB conversations missing raw file, pending tool calls, and raw-vs-DB tool counts are reported before memory distillation trusts Gemini history."
       "Cursor/watermark advancement MUST happen after durable DB write acknowledgement, never before."
       "ClaudeCode ~/.claude/history.jsonl is a prompt-only historical source: import it as conversation_type=history_prompt, chat_type=history_jsonl, source=claude_code, speaker=human_user, authority=claude_history_prompt, and deterministic message_uuid=claude-history:<sha>; it MUST NOT be mistaken for assistant/tool transcript coverage."
       "ClaudeCode historical import MUST refresh conversations.message_count from actual inserted conversation_messages after import because database triggers/upserts can otherwise leave placeholder counts that make Logs and exports report double messages."
       "ClaudeCode conversation normalization MUST maintain a non-destructive overlay: conversation_source_state records current/missing-stale/path-mismatch/raw-only-local-command/raw-only-provider-prompt/raw-only-uningested source evidence, message_labels canonical_state marks exact role/timestamp/content duplicates as equivalent-duplicate, raw_role_state distinguishes native/reconstructed/provider-derived/ambiguous, and no provider JSONL row is physically deleted by normalization."
       "True-user utterance export MUST include ClaudeCode history_prompt rows and exclude equivalent-duplicate, worker/subagent/compaction, task-bound sessions, MissionD runtime prompts, provider context, terminal artifacts, and local-command artifacts; verification must fail if BoardTask/Swarm prompt signatures leak into the export."
       "ClaudeCode provider role normalization MUST be shared by realtime watcher, per-session reconcile, and daily reconcile paths: top-level raw_role=user inside automated slot sessions normalizes to worker_user, interactive Jarvis/user conversations remain user, sidechain progress remains agent_user/agent_assistant, and raw_role is preserved for audit."
       "Historical ClaudeCode role repair is dry-run/report-first through scripts/report-claude-role-attribution.mjs; first pass reports suspected system/user/agent_user drift and never mutates DB."
       "Provider-aware conversation_type classification MUST live behind crates/missiond-core/src/db/conversation_query.rs::classify_conversation_type so ClaudeCode, Codex CLI, and Gemini CLI workers share one rule set: slot-bound sessions (any provider) classify as worker with durable slotId/taskId linkage; background-ingested Codex threads classify as codex_chat (parallel to gemini_chat), never as the human user fallthrough; real human Jarvis user sessions remain user."
       "ClaudeCode slot session capture and reconcile MUST bind conversations.task_id from the currently running BoardTask claimed by that slot after session UUID discovery and after lazy JSONL conversation creation, so mission_conversation_query(taskId=...) works while workers are still running rather than only after final evidence."
       "Codex CLI background ingestion MUST call classify_conversation_type AND preserve the provider role into raw_role so the conversation row carries enough metadata for audit_classification and the role-attribution report; the legacy hardcoded conversation_type=\"user\" + raw_role=None pattern is forbidden."
       "Codex CLI background ingestion MUST refresh conversation message_count from actual inserted rows after each import so the conversation list, Logs, and memory distillation samples do not regress to the initial upsert placeholder count."
       "Historical row classification repair is dry-run/report-first through db::conversation_query::audit_historical_classification: it returns HistoricalClassificationFinding values for codex_user_without_slot, codex_slot_not_worker, worker_loses_slot_linkage, codex_raw_role_missing, and claude_worker_prompt_signature; mission_conversation_query(action=audit_classification) reports candidates without mutation, mission_conversation_query(action=backfill_classification, apply=true) may apply only high-confidence repairs through set_conversation_type, backfill_missing_raw_roles_for_session for old Codex rows, and then rebuild conversation_turns via rebuild_session_turns."
       "Historical ClaudeCode message-role repair is also dry-run/report-first: mission_conversation_query(action=audit_message_roles) reports worker-session rows where source=claude_code, conversation_type=worker, role=user, raw_role=user, and content matches local-command or worker-prompt signatures; mission_conversation_query(action=backfill_message_roles, apply=true) may rewrite only those reviewed rows to worker_user and then rebuild conversation_turns via rebuild_session_turns. It MUST NOT delete provider messages or bulk-relabel real human Jarvis/user conversations."
       "Conversation turn repair is explicit and bounded: mission_conversation_query(action=turn_backfill, sessionId=...) clears only that session's conversation_turns and re-runs tagger_chunker on its canonical message stream; it does not rewrite raw provider logs."]
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
       "recognize_claude_code Blocked MUST require explicit confirmation/model-picker UI (Enter to confirm, Do you want to proceed/make this edit/allow/use this api key, Select model, approval request); the bare words `approval` or `permission(s)` -- including the `bypass permissions on` composer-mode footer toggle and historical task-brief prose -- MUST NOT trigger Blocked on Idle or completed screens."
       "ClaudeCode worker MCP reconnect MUST follow `/mcp` -> Enter -> ArrowDown until missiond -> Enter -> Enter using arrow-key keystrokes only; numeric shortcut selection is forbidden because Claude Code's MCP picker numeric shortcuts have shifted between releases. The keystroke sequence is the SSOT and missiond-pty Session::mcp_reconnect_sequence MUST project from it."
       "When a ClaudeCode worker advertises supports_mcp=true but its mounted tool list does not include any mission_* tool after slot ready, master_control MUST file a durable claude_code_mcp_missing incident so the resident master is woken; if the /mcp arrow-key reconnect ritual does not surface mission_* tools within the policy budget, a follow-up claude_code_mcp_reconnect_failed incident is required, never a silent retry loop."]
    :checker "node scripts/check-v3-pty-recognition-isomorphism.mjs")

  (ops-infra
    :desc "Lisp-owned operational scripts for deploy, smoke, and formatter-converged source hygiene."
    :scripts [scripts/deploy-daemon.sh scripts/rustfmt-missiond.sh scripts/cargo-fmt-touched.sh]
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
       "M6 MissionD formatting MUST be converged: scripts/rustfmt-missiond.sh --check is the repository-owned Rust formatter gate for crates/**."
       "Rust formatter edition MUST be derived from workspace Cargo.toml; ad-hoc rustfmt --edition overrides are forbidden because Rust edition migration is an explicit codebase migration, not a formatter flag choice."
       "No MissionD Rust source may carry formatter exemption markers; legacy rustfmt exemptions are incompatible with MissionD M6."
       "rustfmt MUST run with skip_children=true so formatting a crate root cannot recursively churn child modules outside the intended formatter scope."
       "Rust formatting for external or non-M6 projects MAY remain scoped through scripts/cargo-fmt-touched.sh, including staged, unstaged, and branch-diff modes."
       "The no-Rust-files path MUST exit 0 under set -euo pipefail; filters must not turn an empty grep match into a script failure."
       "MissionD primary runtime database MUST be PostgreSQL-only; the old MissionD SQLite backend, SQLite-to-Postgres migration module, and sqlite feature cfg MUST be absent from active code/build paths."
       "SQLite references are allowed only for external provider durable sources such as Codex CLI state_5.sqlite, or for independent non-MissionD storage crates such as skill-store; they MUST NOT reintroduce a MissionD runtime database backend."]
    :checks ["bash -n scripts/deploy-daemon.sh"
             "bash -n scripts/rustfmt-missiond.sh"
             "scripts/rustfmt-missiond.sh --check"
             "bash -n scripts/cargo-fmt-touched.sh"
             "scripts/cargo-fmt-touched.sh --check"
             "node scripts/check-v3-ops-infra-isomorphism.mjs"])

  (include "shards/v2-convergence-map.lisp")

  (include "shards/pillar-flow-map.lisp")

  (include "shards/implementation-map.lisp")

  (compression-contract
    :v1 "Organized by .missiond/v1/manifest.lisp; root files remain compatibility paths."
    :v2 "Kept as historical source index, implementation status, and wave evidence."
    :v3 "Small executable contracts only: request, artifact, state-machine, policy, genome, pillar-flow-map, implementation map."
    :checks ["node scripts/check-lisp-blueprint-compression.mjs"
             "node scripts/check-architecture-lisp.mjs --no-structure .missiond/v3/missiond-blueprint.lisp"
             "node scripts/check-typed-lisp-compiler.mjs"
             "node scripts/check-v3-pillar-flow-schema.mjs"
             "node scripts/check-v3-v2-coverage.mjs"
             "node scripts/check-v3-runtime-path-hygiene.mjs"
             "node scripts/check-v3-conversation-ingestion-isomorphism.mjs"
             "node scripts/check-v3-cli-conversation-ingestion-isomorphism.mjs"
             "node scripts/check-v3-pty-recognition-isomorphism.mjs"
             "node scripts/check-v3-capability-governance-isomorphism.mjs"
             "node scripts/check-v3-mechanic-boundary-isomorphism.mjs"
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
             "node scripts/check-v3-work-order-lifecycle-isomorphism.mjs"
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
             "node scripts/check-v3-control-plane-m6-split.mjs"
             "node scripts/check-v3-master-control-isomorphism.mjs"
             "node scripts/check-v3-direct-code-drift-policy.mjs"
             "node scripts/check-v3-genome-runtime-isomorphism.mjs"
             "node scripts/check-v3-autopilot-genome-isomorphism.mjs"
             "node scripts/check-v3-commit-convergence-loop.mjs"
             "node scripts/check-v3-nightly-evolution-isomorphism.mjs"
             "node scripts/check-v3-autopilot-runtime-isomorphism.mjs"
             "node scripts/check-v3-workstation-dispatch-isomorphism.mjs"
             "node scripts/check-v3-board-isomorphism.mjs"
             "node scripts/check-frontend-board-lisp-schema.mjs"
             "node scripts/check-frontend-board-code-isomorphism.mjs"
             "node scripts/check-frontend-board-runtime-projection.mjs"
	             "node scripts/check-v3-ops-infra-isomorphism.mjs"
	             "node scripts/check-v3-service-extraction-isomorphism.mjs"
	             "node scripts/check-v3-shared-memory-isomorphism.mjs"
             "node scripts/check-v3-request-flow-smoke.mjs"
             "node scripts/check-v3-code-isomorphism-complete.mjs"
             "node scripts/check-v3-final-convergence.mjs"]
    :rule "New runtime work should cite v3 first, then v2 source-index for historical evidence."))
