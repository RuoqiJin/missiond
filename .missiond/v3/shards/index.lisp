(missiond-blueprint-shards
  :schema "missiond.blueprint-shard-index.v1"
  :root ".missiond/v3/missiond-blueprint.lisp"
  :status active-authoring-index
  :rule "The root V3 blueprint remains the executable compiler input. Shards are scoped authoring indexes that keep worker context small; runtime behavior must not depend on a shard until the root blueprint and compiled projections expose the same contract."

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
