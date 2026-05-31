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

    (artifact work-order
      :schema "missiond.work-order.v1"
      :path ".missiond/work-orders/<work_order_id>/intent.lisp"
      :ssot true
      :writer missiond-work-order.start
      :required [:work_order_id :objective :intent :plan_ref :accepted_shard_id :read_scope :write_scope :audit]
      :invariant "External user, Board, Jarvis, Codex, and ClaudeCode code changes must be tied to a work-order before commit/merge/deploy gates accept code-like diffs.")

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

    (artifact task-result-artifact
      :schema "missiond.task-result-artifact.v1"
      :path "shared-memory://artifacts/task-result/<task_id>/<content_hash>"
      :ssot true
      :writer mission_shared_memory.task_result_put
      :required [:task_id :project_id :summary :content :result_status :result_kind :producer :content_hash :evidence_refs :created_at]
      :invariant "Hard cutover: completion and BoardTask status=done MUST read an existing canonical completed task-result-artifact. worker_settle(done) MUST carry artifact_hash; task_result_put validates producer/result/evidence, plus verification and changed-path evidence for completed write-scoped work. Textual finals, Board notes, PTY/provider output, Markdown descriptions, and Jarvis summary streams are evidence/projections only and MUST NOT synthesize canonical completion. See blueprint note task-result-artifact-hard-cutover.")

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
              ".missiond/research/memory-review*/**"
              ".missiond/research/board-cleanup/**"
              ".missiond/research/kb-memory-triage-*/**"]
      :catalog runtime_artifacts
      :examples [".missiond/v3/runtime/lisp-code-sync/*.report.lisp"
                 ".missiond/v3/runtime/nightly-evolution/*.report.lisp"
                 ".missiond/v3/runtime/jarvis-smoke/*.json"
                 ".missiond/v3/runtime/genome/*.json"
                 ".missiond/v3/runtime/self-evolution/*.proposal.lisp"
                 ".missiond/v3/runtime/master-control/context-packs/*.lisp"
                 ".missiond/v3/runtime/compiled/*.json"
                 "$MISSIOND_RUNTIME_DIR/compiled/*.json"]
      :rule "Cold runtime artifacts are diagnostic/query targets, not authoring SSOT, and are indexed in Postgres runtime_artifacts. Production deploy writes generated projections under MISSIOND_RUNTIME_DIR, defaulting to $HOME/.missiond/runtime/<repo-id>; repo .missiond/v3/runtime/** is dev compatibility only.")
    :invariants
      ["Tools that answer 'what does the SSOT say?' MUST search active-authoring first and exclude cold-runtime by default."
       "Generated compiled JSON and runtime reports are projections/evidence; production runtime paths live outside git via MISSIOND_RUNTIME_DIR."
       "MissionD may query cold-runtime for trace/debug/report lookup, but that query must be explicit and visible in the context-pack."
       "Repo-local search ignore projections (.ignore and nested .missiond/.ignore files) MUST mirror cold-runtime defaults so broad rg/fd searches from repo root or .missiond subroots exclude cold evidence unless --no-ignore, include_runtime=true, or a concrete trace path is explicit."
       "runtime_artifacts retention marks/prunes diagnostic caches only; canonical task/plan evidence is indexed without automatic deletion."]
    :checker "node scripts/check-v3-runtime-path-hygiene.mjs")

  (compiler-plane
    :schema "missiond.compiler-plane.v1"
    :purpose "Keep Lisp as high-density architecture SSOT while preventing every runtime language from becoming a Lisp semantic interpreter."
    :authoring-source ".missiond/v3/**/*.lisp"
    :semantic-compiler missiond-lispc
    :compiler-root "tools/missiond_lispc"
    :runtime-abi ".missiond/v3/runtime/compiled/*.json"
    :compiled-abi ["compiled-v3-blueprint.json"
                   "compiled-runtime-config.json"
                   "compiled-semantic-ir.json"
                   "compiled-agent-slices.json"
                   "compiled-project-agent-navigation.json"
                   "compiled-contract-abi.json"
                   "compiled-project-universe.json"
                   "compiled-workflows.json"
                   "compiled-runtime-control-plane-kernel.json"
                   "compiled-genomes.json"]
    :generated-abi ["crates/missiond-daemon/src/context/v3_contracts/generated.rs"
                    "scripts/generated/v3_contracts.mjs"
                    "scripts/generated/v3_contracts.d.ts"
                    "crates/missiond-daemon/src/context/v3_runtime_defaults/generated.rs"
                    "scripts/generated/v3_runtime_defaults.mjs"]
    :contract-commands [emit-contract-abi emit-plan-contract check-plan-contract]
    :envelope-fields [:schema_version :source_hash :generated_at :diagnostics :payload]
    :payload-fields [:source_units :source_domains :surfaces :functions :artifact_contracts :runtime_policies :checker_registry :plan_contract
                     :production_consumer_boundaries :request_state_projections :scanner_policies :semantic_gates]

    :authority-boundary
      ["missiond-lispc is the only production component allowed to assign Lisp semantics."
       "Generated V3 contract ABI source is tracked in Rust and JS/TS; ignored compiled JSON remains a runtime projection."
       "Rust runtime hot paths consume compiled JSON/runtime config; raw Lisp source fallback is forbidden for production consumers."
       "Plan execution hints and DAG nodes are read from missiond-lispc emit-plan-contract missiond.plan-contract.v2 projection, not ad-hoc Rust keyword scanners."
       "compile/materialization persist plan.contract_json using missiond.plan-contract.v2 shape"
       "plan_dag/parser/validation.rs owns typed plan-contract validation/topological ordering"
       "JS checkers consume semantic-ir/resolved compiler output for active surfaces; JS Lisp parsing is compatibility scaffolding for legacy fixtures and checker migration only."
       "Freshness is source_hash plus source_units plus source_domains; mtime is not semantic authority."]
    :forbidden-production-consumers
      [production-consumer-raw-parser-forbidden
       rust-scan_keyword_pairs
       rust-ad-hoc-colon-keyword-scanner
       js-new-raw-blueprint-parser
       worker-direct-lisp-reader]
    :governance
      (:contract "compiler-plane"
       :checker "node scripts/check-typed-lisp-compiler.mjs"
       :aggregate "node scripts/check-v3-code-isomorphism-complete.mjs"
       :goldens ["tools/missiond_lispc/test/parser_golden.ml"
                 "node scripts/project-v3-contracts.mjs --check --json"
                 "node scripts/compile-v3-runtime.mjs --json"])
    :non-goal "Do not add a second governance layer for the compiler plane; this contract plus typed compiler checks are the boundary.")

  (control-plane-kernel
    :schema "missiond.control-plane-kernel.v1"
    :authority "postgres-typed-runtime-facts"
    :facts [task_contracts task_result_artifacts event_log jobs job_attempts work_leases capability_grants capability_audit_events review_gates board_task_views]
    :runtime-abi-fields [completion_artifact_schema job_state_machine capability_policy sandbox_policy projection_policy]
    :kernel-core [delegate claim-lease capability spawn attempt completion-artifact settle event-log board-projection pty-worker-adapter]
    :optional-full-os-layers [workflow memory skill-store router-experiments codex-replay self-evolution advanced-conversations infra-os advanced-board]
    :feature-gates [MISSIOND_FULL_OS_ENABLE MISSIOND_FEATURE_WORKFLOW_ENABLE MISSIOND_FEATURE_MEMORY_ENABLE MISSIOND_FEATURE_SKILL_STORE_ENABLE MISSIOND_FEATURE_ROUTER_EXPERIMENTS_ENABLE MISSIOND_FEATURE_CODEX_REPLAY_ENABLE MISSIOND_FEATURE_SELF_EVOLUTION_ENABLE MISSIOND_FEATURE_CONVERSATIONS_ENABLE MISSIOND_FEATURE_INFRA_OS_ENABLE MISSIOND_FEATURE_BOARD_ADVANCED_ENABLE]
    :hard-cutover true
    :note "capability_usage.rs is the thin capability-governance facade; capability_usage/runtime.rs owns snapshot/report/candidates/mark/ack; audit.rs owns mission_audit trace/detail/stats/export; codex_ops.rs owns mission_codex_ops recent/thread/tool_stats; tool_directory.rs owns read-only mission_tool_directory list/recommend/lookup/explain/deprecated/guide; agent_navigation.rs owns mission_agent_navigation catalog/review/feedback/suggest_entries."
    :rules ["BoardTask description, Board notes, PTY screens, TUI summaries, and provider prose are projection/observation inputs only."
            "Runtime control paths MUST read task_contracts, subject-bound capability_grants, work_leases, jobs/job_attempts, event_log, and task_result_artifacts; BoardTask.runtime_metadata is a UI/cache projection only."
            "Kernel-internal completion writes MUST call typed task_result_put entrypoints directly; mission_shared_memory JSON action dispatch is an external adapter and MUST NOT be the kernel's reverse dependency."
            "Missing task_contracts on a control-plane task returns TASK_CONTRACT_REQUIRED; MissionD must not parse Markdown descriptions or BoardTask.runtime_metadata to recover canonical control fields."
            "task_result_put and worker_settle MUST pass exact grant_id + subject_kind + subject_id + operation + scope + task_id capability checks; write-scoped completed artifacts also require current attempt_id plus verification and changed-path evidence."
            "A successful worker_settle(done) using an exact settle capability grant MUST atomically mark that grant consumed before terminal Board/job projection; bypass grants remain audit-only and cannot be replayed as exact tokens."
            "Public mission_shared_memory capability_grant MUST itself pass delegate authority: exact authority_grant_id for operation=delegate, or confirmed system/operator/daemon bypass with capability_audit_events. It MUST NOT mint grants from target subject fields alone."
            "Write-scoped completion verification MUST bind to the current job_attempt: attempt.started records a pre worktree manifest, settle-time verification records a post manifest, compares git status plus git diff --name-only between pre/post HEAD, subtracts pre-existing dirty paths, and fails closed when the current attempt or pre manifest is missing."
            "Worker spawn MUST carry an exact subject-bound worker/conversation spawn grant and project sandbox_profile from task_contracts/capability facts; unsupported engine/scope combinations return SANDBOX_POLICY_UNSUPPORTED or CAPABILITY_DENIED."
            "mission_compute_slot(create) for task-bound dynamic workers MUST start a kernel attempt.started event after PTY spawn succeeds and before the worker is advertised idle, so jobs.current_attempt_id and the pre worktree manifest exist before task dispatch or artifact writes."
            "BoardTask claim, release, heartbeat, expiry, and recovery MUST use work_leases as the lease authority; BoardTask claim fields and shared_claims are compatibility projections/mirrors only."
            "ProjectionEngine updates board_task_views and Board-facing status from typed events/state, not from note text or PTY/provider final prose."
            "Non-core full-os tools MUST keep their public MCP names but default to FEATURE_DISABLED unless MISSIOND_FULL_OS_ENABLE or the matching MISSIOND_FEATURE_* gate is explicitly enabled."
            "Startup services for self-evolution, Lisp code sync, workflow recovery, memory embeddings, and multi-provider diagnostics MUST NOT start in kernel-core mode."]
    :state-machine [(job.created created)
                    (job.claimed claimed)
                    (attempt.started running)
                    (observation.recorded running)
                    (artifact.accepted running)
                    (settle.requested running)
                    (job.completed completed)
                    (job.blocked blocked)
                    (job.failed failed)
                    (lease.expired blocked)
                    (capability.denied blocked)]
    :db-migration "crates/missiond-core/migrations/20260527000000_control_plane_kernel.sql"
    :checker "scripts/check-v3-control-plane-kernel-isomorphism.mjs")

  (typed-subplane-contracts
    :schema "missiond.typed-subplane-contracts.v1"
    :forms [surface contract-split domain runtime-projection policy-clause acceptance owner source behavior-universe behavior anchor trigger effect tombstone
            production-consumer-boundary request-state-projection scanner-policy semantic-gate]
    :semantic-ir-facts [contract_split control_plane_domain runtime_policy checker_registry
                        production_consumer_boundary request_state_projection scanner_policy semantic_gate]
    :sidecar-policy "Long prose and historical notes move to .missiond/v3/evidence/blueprint-notes.lisp; active shards keep compiler-readable ids, ownership, runtime projection, checker, and source/evidence anchors."
    :checker "node scripts/check-v3-typed-sidecar-compression.mjs")

  (production-consumer-boundary
    :schema "missiond.production-consumer-boundary.v1"
    (boundary plan-contract-runtime
      :surface mission_plan
      :owner missiond-lispc
      :authority compiled-contract-abi
      :input-schema "missiond.plan-contract.v2"
      :runtime-consumers [mission_plan_execute plan_dag_runtime plan_field_inference]
      :must [consume_plan_contract_json reject_empty_contract reject_rust_compat_contract require_payload_nodes]
      :forbidden [read_raw_plan_sexp_for_execution rust_scan_keyword_pairs production_lisp_scanner])
    (boundary runtime-config-compiled-authority
      :surface runtime_config
      :owner missiond-lispc
      :authority compiled-runtime-config
      :input-schema "missiond.compiled-runtime-config.v1"
      :runtime-consumers [rust_runtime_defaults js_runtime_defaults workstation_runtime_loader]
      :must [consume_compiled_runtime_config validate_source_hash expose_generated_abi]
      :forbidden [raw_blueprint_runtime_fallback direct_lisp_policy_parse]))

  (request-state-projection
    :schema "missiond.request-state-projection.v1"
    (projection mission-request-review
      :surface mission_request
      :owner RequestProjection
      :authority request-local-artifacts
      :runtime-consumers [mission_request_status mission_request_respond]
      :must [consume_artifact_existence consume_projection_target consume_review_events consume_execute_flag emit_review_packet]
      :forbidden [handler_local_review_state_rebuild respond_local_review_state_rebuild]))

  (scanner-policy
    :schema "missiond.scanner-policy.v1"
    (scanner plan-lisp-raw-scanner
      :surface mission_plan
      :owner compatibility-tests
      :status test-and-migration-only
      :allowed-contexts [unit_tests fixtures contract_backfill]
      :must [prefer_missiond_lispc_plan_contract keep_fixture_compatibility]
      :forbidden [mission_plan_execute plan_field_inference dry_run_plan_objective production_consumer]))

  (semantic-gate
    :schema "missiond.semantic-gate.v1"
    (gate final-convergence-semantic-facts
      :surface final_convergence
      :owner missiond-lispc
      :authority compiled-semantic-ir
      :requires [surface function artifact_contract runtime_policy contract_split control_plane_domain source_domain checker_registry agent_entry agent_navigation_policy
                 production_consumer_boundary request_state_projection scanner_policy semantic_gate]
      :must [verify_compiled_semantic_facts treat_text_needles_as_code_anchor_only]
      :forbidden [blueprint_needle_as_semantic_requirement]
      :checker "node scripts/check-v3-final-convergence.mjs --json"))

  (blueprint-shard-index
    :schema "missiond.blueprint-shard-index.v1"
    :index ".missiond/v3/shards/index.lisp"
    :root ".missiond/v3/missiond-blueprint.lisp"
    :status compiler-active
    :rule "The root blueprint remains the compiler entrypoint. Root uses include-shard-index to expand compiler-active shards from shards/index.lisp; shard files still cannot recursively include other shards."
    :shards [request-runtime workstation-runtime control-plane-runtime memory-knowledge-runtime agent-navigation ops-infra v2-convergence-map pillar-flow-map
             universe-service-runtime universe-service-layer-template universe-infrastructure universe-data-residency universe-project-maturity universe-project-registry universe-behavior-closure
             implementation-request-surfaces implementation-execution-surfaces implementation-runtime-surfaces implementation-knowledge-surfaces implementation-ops-surfaces]
    (shard request-runtime
      :path "shards/request-runtime.lisp"
      :status compiler-active)
    (shard workstation-runtime
      :path "shards/workstation-runtime.lisp"
      :status compiler-active)
    (shard control-plane-runtime
      :path "shards/control-plane-runtime.lisp"
      :status compiler-active)
    (shard memory-knowledge-runtime
      :path "shards/memory-knowledge-runtime.lisp"
      :status compiler-active)
    (shard agent-navigation
      :path "shards/agent-navigation.lisp"
      :status compiler-active)
    (shard ops-infra
      :path "shards/ops-infra.lisp"
      :status compiler-active)
    (shard v2-convergence-map
      :path "shards/v2-convergence-map.lisp"
      :status compiler-active)
    (shard pillar-flow-map
      :path "shards/pillar-flow-map.lisp"
      :status compiler-active)
    (shard universe-service-runtime
      :path "shards/universe/service-runtime.lisp"
      :status compiler-active)
    (shard universe-service-layer-template
      :path "shards/universe/service-layer-template.lisp"
      :status compiler-active)
    (shard universe-infrastructure
      :path "shards/universe/infrastructure.lisp"
      :status compiler-active)
    (shard universe-data-residency
      :path "shards/universe/data-residency.lisp"
      :status compiler-active)
    (shard universe-project-maturity
      :path "shards/universe/project-maturity.lisp"
      :status compiler-active)
    (shard universe-project-registry
      :path "shards/universe/project-registry.lisp"
      :status compiler-active)
    (shard universe-behavior-closure
      :path "shards/universe/behavior-closure.lisp"
      :status compiler-active)
    (shard implementation-request-surfaces
      :path "shards/implementation/request-surfaces.lisp"
      :status compiler-active)
    (shard implementation-execution-surfaces
      :path "shards/implementation/execution-surfaces.lisp"
      :status compiler-active)
    (shard implementation-runtime-surfaces
      :path "shards/implementation/runtime-surfaces.lisp"
      :status compiler-active)
    (shard implementation-knowledge-surfaces
      :path "shards/implementation/knowledge-surfaces.lisp"
      :status compiler-active)
    (shard implementation-ops-surfaces
      :path "shards/implementation/ops-surfaces.lisp"
      :status compiler-active))

  (include-shard-index "shards/index.lisp")

  (final-convergence-gate
    :schema "missiond.final-convergence-gate.v1"
    :id "v3-final-convergence"
    :checker "node scripts/check-v3-final-convergence.mjs"
    :required-live-checks [generated-v3-contracts-current
                           typed-lisp-runtime-compile
                           agent-entry-slices
                           agent-navigation-quality
                           agent-navigation-closure
                           runtime-domain-projections
                           control-plane-m6-split
                           production-runtime-boundary]
    :required-split-files ["crates/missiond-daemon/src/handlers/knowledge/plan/compile_authoring.rs"
                           "crates/missiond-daemon/src/handlers/knowledge/plan/approval_review.rs"
                           "crates/missiond-daemon/src/handlers/knowledge/plan/execution_runtime.rs"
                           "crates/missiond-daemon/src/handlers/knowledge/plan/task_runner_dry_run.rs"
                           "crates/missiond-daemon/src/handlers/knowledge/plan/task_contract.rs"
                           "crates/missiond-daemon/src/handlers/knowledge/plan_dag/runtime.rs"
                           "crates/missiond-daemon/src/handlers/knowledge/plan_dag/claim_lease.rs"
                           "crates/missiond-daemon/src/handlers/knowledge/agent_execution/log_surface.rs"
                           "crates/missiond-daemon/src/handlers/knowledge/agent_execution/claim_lease.rs"
                           "crates/missiond-daemon/src/handlers/knowledge/agent_execution/completion_audit.rs"
                           "crates/missiond-daemon/src/handlers/knowledge/workstation_dispatch/descriptor.rs"
                           "crates/missiond-daemon/src/handlers/knowledge/workstation_dispatch/brief.rs"
                           "crates/missiond-daemon/src/handlers/knowledge/workstation_dispatch/decision.rs"
                           "crates/missiond-daemon/src/handlers/knowledge/workstation_dispatch/outcome.rs"
                           "crates/missiond-daemon/src/handlers/knowledge/workstation_dispatch/runner.rs"
                           "crates/missiond-daemon/src/handlers/knowledge/file_artifacts/commit.rs"
                           "crates/missiond-core/src/db/artifact_commit.rs"
                           "crates/missiond-core/src/db/pg/artifact_commit.rs"
                           "crates/missiond-core/migrations/20260523001000_artifact_commit_outbox.sql"]
    (live-check lisp-blueprint-compression
      :argv ["scripts/check-lisp-blueprint-compression.mjs"] :timeout-ms 60000)
    (live-check architecture-lisp
      :argv ["scripts/check-architecture-lisp.mjs" "--no-structure" ".missiond/v3/missiond-blueprint.lisp"] :timeout-ms 60000)
    (live-check v3-code-isomorphism-complete
      :argv ["scripts/check-v3-code-isomorphism-complete.mjs" "--json"] :json true :timeout-ms 120000)
    (live-check v2-public-coverage
      :argv ["scripts/check-v3-v2-coverage.mjs" "--json"] :json true :timeout-ms 60000)
    (live-check task-contract-all
      :argv ["scripts/check-task-contract.mjs" "--all"] :timeout-ms 120000)
    (live-check missiond-blue-green-deploy
      :argv ["scripts/check-missiond-blue-green-deploy.mjs" "--json"] :json true :timeout-ms 60000)
    (live-check missiond-rustfmt-convergence
      :command "bash" :argv ["scripts/rustfmt-missiond.sh" "--check"] :timeout-ms 120000)
    (live-check project-ssot-universe
      :argv ["scripts/check-project-ssot-universe.mjs" "--json"] :json true :timeout-ms 60000)
    (live-check v3-behavior-closure
      :argv ["scripts/check-v3-behavior-closure.mjs" "--json"] :json true :timeout-ms 60000)
    (live-check infrastructure-universe
      :argv ["scripts/check-v3-infrastructure-universe-isomorphism.mjs" "--json"] :json true :timeout-ms 60000)
    (live-check data-residency-universe
      :argv ["scripts/check-v3-data-residency-universe-isomorphism.mjs" "--json"] :json true :timeout-ms 60000)
    (live-check interaction-gateway
      :argv ["scripts/check-v3-interaction-gateway-isomorphism.mjs" "--json"] :json true :timeout-ms 60000)
    (live-check interaction-ledger
      :argv ["scripts/check-v3-interaction-ledger-isomorphism.mjs" "--json"] :json true :timeout-ms 60000)
    (live-check project-maturity
      :argv ["scripts/check-project-maturity.mjs" "--min-level" "M5"] :timeout-ms 60000)
    (live-check auth-m6-depth
      :argv ["scripts/check-project-maturity.mjs" "--min-level" "M6" "--project" "auth" "--json"] :json true :timeout-ms 60000)
    (live-check generated-v3-contracts-current
      :argv ["scripts/project-v3-contracts.mjs" "--check" "--json"] :json true :timeout-ms 60000)
    (live-check typed-lisp-runtime-compile
      :argv ["scripts/compile-v3-runtime.mjs" "--check" "--json"] :json true :timeout-ms 60000)
    (live-check agent-entry-slices
      :argv ["scripts/check-v3-agent-entry-slices.mjs" "--json"] :json true :timeout-ms 60000)
    (live-check agent-navigation-quality
      :argv ["scripts/check-v3-agent-navigation-quality.mjs" "--json" "--check"] :json true :timeout-ms 60000)
    (live-check agent-navigation-closure
      :argv ["scripts/check-v3-agent-navigation-closure.mjs" "--json"] :json true :timeout-ms 60000)
    (live-check runtime-domain-projections
      :argv ["scripts/check-v3-runtime-domain-projections.mjs" "--json"] :json true :timeout-ms 60000)
    (live-check control-plane-m6-split
      :argv ["scripts/check-v3-control-plane-m6-split.mjs" "--json"] :json true :timeout-ms 60000)
    (live-check production-runtime-boundary
      :argv ["scripts/check-v3-production-runtime-boundary.mjs" "--json"] :json true :timeout-ms 60000)
    (live-check semantic-checker-coverage
      :argv ["scripts/check-v3-semantic-checker-coverage.mjs" "--json"] :json true :timeout-ms 60000)
    (live-check runtime-artifact-catalog
      :argv ["scripts/check-v3-runtime-artifact-catalog.mjs" "--json"] :json true :timeout-ms 60000)
    (live-check typed-sidecar-compression
      :argv ["scripts/check-v3-typed-sidecar-compression.mjs" "--json"] :json true :timeout-ms 60000)
    (runtime-check cargo-test-workspace
      :command "cargo" :argv ["test" "--workspace"] :timeout-ms 900000)
    (runtime-check board-build
      :command "pnpm" :argv ["--dir" "packages/board" "build"] :timeout-ms 300000)
    (runtime-check git-diff-check
      :command "git" :argv ["diff" "--check"] :timeout-ms 60000)
    (blueprint-needle v2-convergence-map :needle "(v2-convergence-map")
    (blueprint-needle public-surface-map :needle "(public-surface-map")
    (blueprint-needle pillar-flow-map :needle "(pillar-flow-map")
    (blueprint-needle implementation-map :needle "(implementation-map")
    (blueprint-needle workstation-config :needle "(workstation-config")
    (blueprint-needle control-plane-m6-split :needle "(control-plane-m6-split")
    (blueprint-needle control-plane-split-checker :needle "check-v3-control-plane-m6-split.mjs")
    (blueprint-needle data-residency-universe :needle "(data-residency-universe")
    (blueprint-needle data-residency-checker :needle "check-v3-data-residency-universe-isomorphism.mjs")
    (blueprint-needle lisp-code-sync-loop :needle "(lisp-code-sync-loop")
    (blueprint-needle lisp-code-sync-checker :needle "check-v3-lisp-code-sync-isomorphism.mjs")
    (blueprint-needle context-pack-run-wave :needle "context-pack-run-wave")
    (blueprint-needle context-pack-dispatch-policy :needle "dispatch-policy context-pack-run-wave")
    (blueprint-needle swarm-run-resolves-project-root :needle "mission_swarm_run MUST resolve project_id to a registered project_root")
    (blueprint-needle autopilot-overrides-cwd :needle "Autopilot ensure_pty MUST override pty_slot.cwd")
    (blueprint-needle durable-conversation-taskid :needle "bind conversations.task_id to the active BoardTask via a bounded retry helper")
    (blueprint-needle taskid-query-survives-slot-reuse :needle "message-anchored BoardTask id fallback")
    (blueprint-needle runtime-v3-paths :needle ".missiond/v3/runtime/")
    (blueprint-needle v2-historical-status :needle ":v2 \"Kept as historical")
    (blueprint-needle entry-shape :needle ":entry [")
    (blueprint-needle core-shape :needle ":core (")
    (blueprint-needle egress-shape :needle ":egress [")
    (blueprint-needle artifact-commit-outbox :needle "artifact_commit_outbox recovery")
    (facade-budget mission_plan-facade
      :file "crates/missiond-daemon/src/handlers/knowledge/plan.rs" :max-lines 800)
    (facade-budget mission_execution-facade
      :file "crates/missiond-daemon/src/handlers/knowledge/agent_execution.rs" :max-lines 350)
    (facade-budget workstation_dispatch-facade
      :file "crates/missiond-daemon/src/handlers/knowledge/workstation_dispatch.rs" :max-lines 400)
    (runtime-file workstation-runtime-loader
      :file "scripts/lib/v3_workstation_runtime.mjs"
      :needles ["DEFAULT_WORKSTATION_RUNTIME_CONFIG" "compiled-runtime-workstation.json" "contextPackDispatchPolicy"
                "V3_BLUEPRINT_CONFIG_ERROR" "source_units"
                "Raw V3 Lisp source fallback is not a production runtime path"
                "compiled runtime config is required"])
    (runtime-file runtime-domain-projection-checker
      :file "scripts/check-v3-runtime-domain-projections.mjs"
      :needles ["compiled-runtime-workstation.json" "missiond.compiled-runtime-domain.v1"
                "RUNTIME_DOMAIN_FILES"])
    (runtime-file final-convergence-manifest
      :file ".missiond/v3/runtime/compiled/compiled-final-convergence-manifest.json"
      :needles ["missiond.compiled-final-convergence-manifest.v1"
                "runtime-domain-projections"])
    (runtime-file project-agent-navigation-artifact
      :file ".missiond/v3/runtime/compiled/compiled-project-agent-navigation.json"
      :needles ["missiond.compiled-project-agent-navigation.v1"
                "read-only-registered-projects"
                "coverageState"])
    (runtime-file behavior-navigation-artifact
      :file ".missiond/v3/runtime/compiled/compiled-behavior-navigation.json"
      :needles ["missiond.compiled-behavior-navigation.v1" "anchors"])
    (runtime-file artifact-commit-outbox-helper
      :file "crates/missiond-daemon/src/handlers/knowledge/file_artifacts/commit.rs"
      :needles ["ArtifactCommitEnvelope" "operation_key" "artifact_commit_outbox_mark_complete"
                "recover_artifact_commit_outbox" "artifact sha mismatch"])
    (runtime-file context-pack-run-wave
      :file "scripts/context-pack-run-wave.mjs"
      :needles ["loadWorkstationRuntimeConfigForRepo" "contextPackMaxParallel" "ensureWaveLedgers" "--apply"])
    (runtime-file context-pack-materialize-wave
      :file "scripts/context-pack-materialize-wave.mjs"
      :needles ["context-pack-run-wave.mjs" "loadWorkstationRuntimeConfigForRepo" "model_profile" "timeout_secs"])
    (runtime-file task-runner-dispatch
      :file "scripts/task-runner-dispatch.mjs"
      :needles ["loadWorkstationRuntimeConfigForRepo" "model_profile" "timeout_secs"])
    (runtime-file task-runner-submit-dispatch
      :file "scripts/task-runner-submit-dispatch.mjs"
      :needles ["runDispatch" "runtime_projection" "model_profile" "timeout_secs"]))

  (compression-contract
    :v1 "Organized by .missiond/v1/manifest.lisp; root files remain compatibility paths."
    :v2 "Kept as historical source index, implementation status, and wave evidence."
    :v3 "Small executable contracts only: request, artifact, state-machine, policy, genome, pillar-flow-map, implementation map."
    :checks ["node scripts/check-lisp-blueprint-compression.mjs"
             "node scripts/check-architecture-lisp.mjs --no-structure .missiond/v3/missiond-blueprint.lisp"
             "node scripts/check-typed-lisp-compiler.mjs"
             "node scripts/check-v3-pillar-flow-schema.mjs"
             "node scripts/check-v3-v2-coverage.mjs"
             "node scripts/check-v3-behavior-closure.mjs"
             "node scripts/check-v3-runtime-path-hygiene.mjs"
             "node scripts/check-v3-production-runtime-boundary.mjs"
             "node scripts/check-v3-runtime-domain-projections.mjs"
             "node scripts/check-v3-architecture-boundaries.mjs"
             "node scripts/check-v3-semantic-checker-coverage.mjs"
             "node scripts/check-v3-agent-entry-slices.mjs"
             "node scripts/check-v3-agent-navigation-quality.mjs"
             "node scripts/check-v3-agent-navigation-closure.mjs"
             "node scripts/check-v3-runtime-artifact-catalog.mjs"
             "node scripts/check-v3-typed-sidecar-compression.mjs"
             "node scripts/check-v3-conversation-ingestion-isomorphism.mjs"
             "node scripts/check-v3-cli-conversation-ingestion-isomorphism.mjs"
             "node scripts/check-v3-pty-recognition-isomorphism.mjs"
             "node scripts/check-v3-conversation-session-management.mjs"
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
	             "node scripts/check-v3-control-plane-kernel-isomorphism.mjs"
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
