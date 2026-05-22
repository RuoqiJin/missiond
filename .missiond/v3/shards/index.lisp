(missiond-blueprint-shards
  :schema "missiond.blueprint-shard-index.v1"
  :root ".missiond/v3/missiond-blueprint.lisp"
  :status compiler-active-index
  :rule "The root V3 blueprint is the compiler entrypoint. This index is a review manifest only; root direct includes are the executable shard topology, and recursive shard includes are forbidden."

  (shard request-runtime
    :status compiler-active
    :path "shards/request-runtime.lisp"
    :root-include "(include \"shards/request-runtime.lisp\")"
    :surfaces [mission_request unified-entry-runtime file-artifacts source-hygiene context-pack review-gate task-runner-cli])

  (shard workstation-runtime
    :status compiler-active
    :path "shards/workstation-runtime.lisp"
    :root-include "(include \"shards/workstation-runtime.lisp\")"
    :surfaces [workstation-config workstation-pool workstation-dispatch autopilot-runtime codex-boot-context])

  (shard control-plane-runtime
    :status compiler-active
    :path "shards/control-plane-runtime.lisp"
    :root-include "(include \"shards/control-plane-runtime.lisp\")"
    :surfaces [resident-master-control lisp-code-drift-policy commit-lisp-convergence-loop lisp-code-sync-loop nightly-evolution-loop cascade-governance router-policy compute-primitives])

  (shard project-universe
    :status compiler-active
    :path "shards/project-universe.lisp"
    :root-include "(include \"shards/project-universe.lisp\")"
    :surfaces [project-registry data-residency-universe eventbridge eventhub-service-boundary])

  (shard memory-knowledge-runtime
    :status compiler-active
    :path "shards/memory-knowledge-runtime.lisp"
    :root-include "(include \"shards/memory-knowledge-runtime.lisp\")"
    :surfaces [memory-kb memory-provider-boundary skill-runtime conversation-ingestion capability-governance incident-governance evidence-governance-view])

  (shard ops-infra
    :status compiler-active
    :path "shards/ops-infra.lisp"
    :root-include "(include \"shards/ops-infra.lisp\")"
    :surfaces [ops-infra missiond-blue-green-self-update])

  (shard v2-convergence-map
    :status compiler-active
    :path "shards/v2-convergence-map.lisp"
    :root-include "(include \"shards/v2-convergence-map.lisp\")"
    :invariant "V2 convergence coverage is compiled through missiond-lispc resolver and remains part of final convergence.")

  (shard pillar-flow-map
    :status compiler-active
    :path "shards/pillar-flow-map.lisp"
    :root-include "(include \"shards/pillar-flow-map.lisp\")"
    :invariant "Pillar function entry/core/egress facts are compiled through semantic IR with shard source locations.")

  (shard implementation-map
    :status compiler-active
    :path "shards/implementation-map.lisp"
    :root-include "(include \"shards/implementation-map.lisp\")"
    :invariant "Implementation surfaces are compiled through semantic IR with shard source locations."))
