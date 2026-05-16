(work-order-intent
  :schema "missiond.work-order.intent.v1"
  :id "20260516-codex-boot-work-order-gate"
  :objective "Implement Codex boot context capsule and external work-order gate"
  :source external-codex
  :status accepted
  :unknowns []
  :evidence_refs [".missiond/v3/evidence/codex-boot-context.lisp"
                  ".missiond/workflows/work-order-lifecycle.lisp"
                  ".githooks/pre-commit"]
  :constraints ["Lisp-first" "no-secret-values" "commit-through-work-order-gate"])
