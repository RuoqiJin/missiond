;; Wave 41 dispatch-time pattern cards.

(pattern-cards wave41-v3-complete-isomorphism-gate-v0
  :schema "missiond.pattern-cards.dispatch.v0"
  :wave wave41

  (card status-graduation-is-a-gate
    :use-for [wave41-01-v3-complete-isomorphism-gate-v0]
    :summary "Do not merely rename status strings; add a checker that makes the renamed status enforceable."
    :recipe ["Update the blueprint first so Lisp declares the complete state."
             "Update per-surface checkers from partial needles to code-aligned needles."
             "Add a new aggregate checker that fails if any implementation-map surface is still partial."
             "Run every per-surface checker live from the aggregate gate so code-aligned means the code evidence still holds."]
    :known-good ["scripts/check-v3-task-lifecycle-isomorphism.mjs :: checkFiles"
                 "scripts/check-lisp-blueprint-compression.mjs :: validateV3BlueprintAst"])

  (card aggregate-checker-shape
    :use-for [wave41-01-v3-complete-isomorphism-gate-v0]
    :summary "A completion checker should be read-only, JSON-capable, and useful alone in CI."
    :recipe ["Support --dry-fixture with one good fixture and at least two bad fixtures: missing surface and partial status."
             "Support --json with {ok, diagnostics, surfaces, checks} or a similarly stable shape."
             "Use node child_process spawnSync only for deterministic local checker commands; never shell through a string."
             "Report per-surface checker failures with the command name and stderr/stdout tail."]
    :known-good ["scripts/check-v3-workstation-config-isomorphism.mjs :: requireAll"
                 "scripts/check-task-runner-manifest.mjs :: JSON diagnostics"])

  (card no-runtime-drift
    :use-for [wave41-01-v3-complete-isomorphism-gate-v0]
    :summary "This wave is a Lisp/checker closeout, not a Rust or frontend implementation wave."
    :recipe ["Do not edit crates/** or packages/**."
             "If a per-surface checker fails after the status change, fix the checker/fixture expectation only when the live implementation already satisfies the V3 note."
             "If real implementation evidence is missing, leave that surface partial and report it instead of forcing the graduation."]
    :known-good ["node scripts/check-v3-request-lisp-isomorphism.mjs"
                 "node scripts/check-v3-workstation-config-isomorphism.mjs"]))
