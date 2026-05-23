(work-order-intent
  :schema "missiond.work-order.intent.v1"
  :id "20260523-v3-ssot-compiled-boundary"
  :objective "Implement V3 SSOT compiled runtime boundary and runtime artifact catalog"
  :source external-codex
  :status accepted
  :unknowns []
  :evidence_refs ["node scripts/project-v3-contracts.mjs --check --json"
                  "node scripts/compile-v3-runtime.mjs --check --json"
                  "node scripts/check-typed-lisp-compiler.mjs --json"
                  "node scripts/check-v3-code-isomorphism-complete.mjs --json"
                  "node scripts/check-v3-runtime-path-hygiene.mjs --json"
                  "node scripts/check-v3-final-convergence.mjs --json --static-only"
                  "cargo test -p missiond-daemon v3_blueprint_runtime"
                  "cargo test -p missiond-daemon shared_memory::tests::runtime_artifact"
                  "git diff --check"]
  :constraints ["Lisp-first" "no-secret-values" "commit-through-work-order-gate"])
