(work-order-intent
  :schema "missiond.work-order.intent.v1"
  :id "20260522-v3-ssot-projection"
  :objective "Converge V3 SSOT typed projection checks and runtime fallback diagnostics"
  :source external-codex
  :status accepted
  :unknowns []
  :evidence_refs ["node scripts/check-v3-final-convergence.mjs --json"
                  "cargo test -p missiond-daemon v3_blueprint_runtime"
                  "git diff --check"]
  :constraints ["Lisp-first" "no-secret-values" "commit-through-work-order-gate"])
