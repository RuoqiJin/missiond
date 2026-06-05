(work-order-intent
  :schema "missiond.work-order.intent.v1"
  :id "wo-20260605162444-Add-Codex-App-context-pack-regis"
  :objective "Add Codex App context pack registry"
  :source external-codex
  :status accepted
  :unknowns []
  :evidence_refs [".missiond/v3/evidence/codex-boot-context.lisp"
                  ".missiond/v3/shards/memory-knowledge-runtime.lisp"
                  "scripts/mission-context-pack.mjs"
                  "scripts/check-v3-context-surface-registry.mjs"]
  :constraints ["Lisp-first" "no-secret-values" "commit-through-work-order-gate"])
