(missiond-blueprint-shards
  :schema "missiond.blueprint-shard-index.v1"
  :root ".missiond/v3/missiond-blueprint.lisp"
  :status compiler-active-index
  :rule "The root V3 blueprint is the compiler entrypoint. Shards referenced through root include forms are executable SSOT source units; runtime behavior must depend on compiled projections, not raw shard reads."

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
    :invariant "Implementation surfaces are compiled through semantic IR with shard source locations.")

  (shard typed-compiler-runtime-projection
    :status active
    :surfaces [typed-lisp-compiler semantic-ir-compiler runtime-load-explanation]
    :owns [missiond-lispc-emit-v3 missiond-lispc-emit-semantic-ir compiled-runtime-status js-checker-compiled-contract-loader rust-v3-source-fallback-diagnostics]
    :code ["scripts/lib/v3_compiled_contract.mjs"
           "scripts/check-v3-code-isomorphism-complete.mjs"
           "scripts/check-v3-request-lisp-isomorphism.mjs"
           "scripts/check-v3-pillar-flow-schema.mjs"
           "scripts/check-v3-v2-coverage.mjs"
           "scripts/check-v3-final-convergence.mjs"
           "crates/missiond-daemon/src/context/v3_blueprint_runtime.rs"]
    :invariant "Live JS checkers consume typed/semantic compiler projections for surface and function facts; Rust runtime reports whether it loaded compiled V3, source Lisp fallback, or embedded test/no-install defaults.")

  (shard workstation-policy-shards
    :status indexed-in-root
    :surfaces [workstation-config workstation-pool workstation-dispatch autopilot-runtime]
    :root-anchor "workstation-policy-shards"
    :invariant "Worker routing, slot templates, timeouts, exact-shard gates, and prompt contracts are reviewed as separate policies even while the compiler input remains the root blueprint.")

  (shard project-universe-shards
    :status indexed-in-root
    :surfaces [project-registry data-residency-universe ops-infra]
    :root-anchor "project-blueprint-registry"
    :invariant "High-change project identity, maturity, data residency, and deploy authority facts should live in project-local blueprints registered from V3 rather than expanding the root monolith."))
