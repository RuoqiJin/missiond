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
      :catalog runtime_artifacts
      :examples [".missiond/v3/runtime/lisp-code-sync/*.report.lisp"
                 ".missiond/v3/runtime/nightly-evolution/*.report.lisp"
                 ".missiond/v3/runtime/jarvis-smoke/*.json"
                 ".missiond/v3/runtime/genome/*.json"
                 ".missiond/v3/runtime/self-evolution/*.proposal.lisp"
                 ".missiond/v3/runtime/master-control/context-packs/*.lisp"
                 ".missiond/v3/runtime/compiled/*.json"]
      :rule "Cold runtime artifacts are diagnostic/query targets, not authoring SSOT. They are indexed in Postgres runtime_artifacts for evidence_view/master-status lookup and excluded from broad rg/review/search unless include_runtime=true or a concrete trace/report path is requested.")
    :invariants
      ["Tools that answer 'what does the SSOT say?' MUST search active-authoring first and exclude cold-runtime by default."
       "Generated compiled JSON and runtime reports are projections/evidence; they must not be treated as editable blueprint source."
       "MissionD may query cold-runtime for trace/debug/report lookup, but that query must be explicit and visible in the context-pack."
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
                   "compiled-contract-abi.json"
                   "compiled-project-universe.json"
                   "compiled-workflows.json"
                   "compiled-genomes.json"]
    :generated-abi ["crates/missiond-daemon/src/context/v3_contracts/generated.rs"
                    "scripts/generated/v3_contracts.mjs"
                    "scripts/generated/v3_contracts.d.ts"]
    :contract-commands [emit-contract-abi emit-plan-contract check-plan-contract]
    :envelope-fields [:schema_version :source_hash :generated_at :diagnostics :payload]
    :payload-fields [:source_units :surfaces :functions :artifact_contracts :runtime_policies :checker_registry :plan_contract]

    :authority-boundary
      ["missiond-lispc is the only production component allowed to assign Lisp semantics."
       "Generated V3 contract ABI source is tracked in Rust and JS/TS; ignored compiled JSON remains a runtime projection."
       "Rust runtime hot paths consume compiled JSON/runtime config; raw Lisp fallback requires MISSIOND_V3_ALLOW_SOURCE_FALLBACK and emits blocking diagnostics otherwise."
       "Plan execution hints are read from missiond-lispc emit-plan-contract projection, not ad-hoc Rust keyword scanners."
       "JS checkers consume semantic-ir/resolved compiler output for active surfaces; JS Lisp parsing is compatibility scaffolding for legacy fixtures and checker migration only."
       "Freshness is source_hash plus source_units; mtime is not semantic authority."]
    :forbidden-production-consumers
      [rust-scan_keyword_pairs
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

  (typed-subplane-contracts
    :schema "missiond.typed-subplane-contracts.v1"
    :forms [surface contract-split domain runtime-projection policy-clause acceptance owner source]
    :semantic-ir-facts [contract_split control_plane_domain runtime_policy checker_registry]
    :sidecar-policy "Long prose and historical notes move to .missiond/v3/evidence/blueprint-notes.lisp; active shards keep compiler-readable ids, ownership, runtime projection, checker, and source/evidence anchors."
    :checker "node scripts/check-v3-typed-sidecar-compression.mjs")

  (blueprint-shard-index
    :schema "missiond.blueprint-shard-index.v1"
    :index ".missiond/v3/shards/index.lisp"
    :root ".missiond/v3/missiond-blueprint.lisp"
    :status compiler-active
    :rule "The root blueprint remains the compiler entrypoint. All compiler-active shards are flat direct includes from root; shards/index.lisp is a review manifest, not a recursive include source."
    :shards [request-runtime workstation-runtime control-plane-runtime project-universe memory-knowledge-runtime ops-infra v2-convergence-map pillar-flow-map implementation-map]
    (shard request-runtime
      :path "shards/request-runtime.lisp"
      :status compiler-active)
    (shard workstation-runtime
      :path "shards/workstation-runtime.lisp"
      :status compiler-active)
    (shard control-plane-runtime
      :path "shards/control-plane-runtime.lisp"
      :status compiler-active)
    (shard project-universe
      :path "shards/project-universe.lisp"
      :status compiler-active)
    (shard memory-knowledge-runtime
      :path "shards/memory-knowledge-runtime.lisp"
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
    (shard implementation-map
      :path "shards/implementation-map.lisp"
      :status compiler-active))

  (include "shards/request-runtime.lisp")

  (include "shards/workstation-runtime.lisp")

  (include "shards/control-plane-runtime.lisp")

  (include "shards/project-universe.lisp")

  (include "shards/memory-knowledge-runtime.lisp")

  (include "shards/ops-infra.lisp")

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
             "node scripts/check-v3-production-runtime-boundary.mjs"
             "node scripts/check-v3-semantic-checker-coverage.mjs"
             "node scripts/check-v3-runtime-artifact-catalog.mjs"
             "node scripts/check-v3-typed-sidecar-compression.mjs"
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
