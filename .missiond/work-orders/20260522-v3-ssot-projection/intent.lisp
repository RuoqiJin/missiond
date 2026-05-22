(work-order-intent
  :schema "missiond.work-order.intent.v1"
  :id "20260522-v3-ssot-projection"
  :objective "Converge V3 SSOT compiler modules, typed projection checks, self-evolution analysis, and runtime fallback diagnostics"
  :source external-codex
  :status accepted
  :unknowns []
  :evidence_refs ["scripts/rustfmt-missiond.sh --check"
                  "dune test --root tools/missiond_lispc"
                  "node scripts/compile-v3-runtime.mjs --json"
                  "node scripts/check-v3-code-isomorphism-complete.mjs --json"
                  "node scripts/analyze-v3-self-evolution.mjs --json"
                  "node scripts/check-v3-final-convergence.mjs --json"
                  "cargo test -p missiond-daemon v3_blueprint_runtime"
                  "cargo test --workspace"
                  "git diff --check"]
  :constraints ["Lisp-first" "no-secret-values" "commit-through-work-order-gate"])
