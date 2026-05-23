(missiond-blueprint-shards
  :schema "missiond.blueprint-shard-index.v1"
  :root ".missiond/v3/missiond-blueprint.lisp"
  :status compiler-active-index
  :rule "The root V3 blueprint is the compiler entrypoint. Root include-shard-index expands this manifest's compiler-active shard paths; recursive shard includes remain forbidden."

  (shard request-runtime
    :status compiler-active
    :domain control-plane-runtime
    :path "shards/request-runtime.lisp"
    :root-include "(include \"shards/request-runtime.lisp\")"
    :surfaces [mission_request unified-entry-runtime file-artifacts source-hygiene context-pack review-gate task-runner-cli])

  (shard workstation-runtime
    :status compiler-active
    :domain workstation-runtime
    :path "shards/workstation-runtime.lisp"
    :root-include "(include \"shards/workstation-runtime.lisp\")"
    :surfaces [workstation-config workstation-pool workstation-dispatch autopilot-runtime codex-boot-context])

  (shard control-plane-runtime
    :status compiler-active
    :domain control-plane-runtime
    :path "shards/control-plane-runtime.lisp"
    :root-include "(include \"shards/control-plane-runtime.lisp\")"
    :surfaces [resident-master-control lisp-code-drift-policy commit-lisp-convergence-loop lisp-code-sync-loop nightly-evolution-loop cascade-governance router-policy compute-primitives])

  (shard memory-knowledge-runtime
    :status compiler-active
    :domain memory-knowledge-runtime
    :path "shards/memory-knowledge-runtime.lisp"
    :root-include "(include \"shards/memory-knowledge-runtime.lisp\")"
    :surfaces [memory-kb memory-provider-boundary skill-runtime conversation-ingestion capability-governance incident-governance evidence-governance-view])

  (shard ops-infra
    :status compiler-active
    :domain ops-infra
    :path "shards/ops-infra.lisp"
    :root-include "(include \"shards/ops-infra.lisp\")"
    :surfaces [ops-infra missiond-blue-green-self-update])

  (shard v2-convergence-map
    :status compiler-active
    :domain v2-convergence
    :path "shards/v2-convergence-map.lisp"
    :root-include "(include \"shards/v2-convergence-map.lisp\")"
    :invariant "V2 convergence coverage is compiled through missiond-lispc resolver and remains part of final convergence.")

  (shard pillar-flow-map
    :status compiler-active
    :domain pillar-flow
    :path "shards/pillar-flow-map.lisp"
    :root-include "(include \"shards/pillar-flow-map.lisp\")"
    :invariant "Pillar function entry/core/egress facts are compiled through semantic IR with shard source locations.")

  (shard universe-service-runtime
    :status compiler-active
    :domain universe
    :path "shards/universe/service-runtime.lisp"
    :root-include "(include \"shards/universe/service-runtime.lisp\")"
    :surfaces [service-runtime-universe eventhub-service-boundary])

  (shard universe-infrastructure
    :status compiler-active
    :domain universe
    :path "shards/universe/infrastructure.lisp"
    :root-include "(include \"shards/universe/infrastructure.lisp\")"
    :surfaces [infrastructure-universe eventbridge])

  (shard universe-data-residency
    :status compiler-active
    :domain universe
    :path "shards/universe/data-residency.lisp"
    :root-include "(include \"shards/universe/data-residency.lisp\")"
    :surfaces [data-residency-universe])

  (shard universe-project-maturity
    :status compiler-active
    :domain universe
    :path "shards/universe/project-maturity.lisp"
    :root-include "(include \"shards/universe/project-maturity.lisp\")"
    :surfaces [project-maturity-model project-maturity-registry])

  (shard universe-project-registry
    :status compiler-active
    :domain universe
    :path "shards/universe/project-registry.lisp"
    :root-include "(include \"shards/universe/project-registry.lisp\")"
    :surfaces [project-registry])

  (shard universe-behavior-closure
    :status compiler-active
    :domain universe
    :path "shards/universe/behavior-closure.lisp"
    :root-include "(include \"shards/universe/behavior-closure.lisp\")"
    :surfaces [behavior-closure])

  (shard implementation-request-surfaces
    :status compiler-active
    :domain implementation-map
    :path "shards/implementation/request-surfaces.lisp"
    :root-include "(include \"shards/implementation/request-surfaces.lisp\")"
    :surfaces [mission_request unified-entry-runtime file-artifacts mission_directive mission_plan review-gate])

  (shard implementation-execution-surfaces
    :status compiler-active
    :domain implementation-map
    :path "shards/implementation/execution-surfaces.lisp"
    :root-include "(include \"shards/implementation/execution-surfaces.lisp\")"
    :surfaces [evidence-collector mission_execution-log mission_execution-claim-lease mission_execution-completion-audit mission_workflow work-order-lifecycle external-work-order-gate task-runner-cli workstation-dispatch])

  (shard implementation-runtime-surfaces
    :status compiler-active
    :domain implementation-map
    :path "shards/implementation/runtime-surfaces.lisp"
    :root-include "(include \"shards/implementation/runtime-surfaces.lisp\")"
    :surfaces [workstation-config workstation-pool resident-master-control commit-lisp-convergence-loop lisp-code-sync-loop lisp-code-sync-storm-circuit nightly-evolution-loop autopilot-runtime genome-runtime mission_board board-search-noise-governance conversation-ingestion])

  (shard implementation-knowledge-surfaces
    :status compiler-active
    :domain implementation-map
    :path "shards/implementation/knowledge-surfaces.lisp"
    :root-include "(include \"shards/implementation/knowledge-surfaces.lisp\")"
    :surfaces [context-pack memory-kb codex-boot-context project-registry data-residency-universe memory-provider-boundary eventhub-service-boundary skill-runtime cascade-governance router-policy incident-governance decision-inbox-revalidation capability-governance mission-shared-memory evidence-governance-view])

  (shard implementation-ops-surfaces
    :status compiler-active
    :domain implementation-map
    :path "shards/implementation/ops-surfaces.lisp"
    :root-include "(include \"shards/implementation/ops-surfaces.lisp\")"
    :surfaces [typed-lisp-compiler semantic-ir-compiler source-hygiene lisp-code-drift-policy eventbridge board-frontend compute-primitives sysinfra-control runtime-load-explanation ops-infra missiond-blue-green-self-update]))
