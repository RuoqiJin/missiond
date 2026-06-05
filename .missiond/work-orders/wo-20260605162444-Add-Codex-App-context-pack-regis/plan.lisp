(work-order-plan
  :schema "missiond.work-order.plan.v1"
  :id "wo-20260605162444-Add-Codex-App-context-pack-regis"
  :intent "wo-20260605162444-Add-Codex-App-context-pack-regis"
  :status accepted
  :accepted_shards
    ((shard default
       :accepted_shard_id "wo-20260605162444-Add-Codex-App-context-pack-regis-shard-default"
       :read_scope ["."]
       :write_scope [
         "AGENTS.md"
         ".missiond/v3/evidence/codex-boot-context.lisp"
         ".missiond/v3/missiond-blueprint.lisp"
         ".missiond/v3/shards/implementation/knowledge-surfaces.lisp"
         ".missiond/v3/shards/index.lisp"
         ".missiond/v3/shards/memory-knowledge-runtime.lisp"
         "scripts/check-v3-code-isomorphism-complete.mjs"
         "scripts/check-v3-codex-boot-context-isomorphism.mjs"
         "scripts/check-v3-context-surface-registry.mjs"
         "scripts/mission-context-pack.mjs"
         ".missiond/work-orders/wo-20260605162444-Add-Codex-App-context-pack-regis"
       ]
       :acceptance [
         "node scripts/check-v3-context-surface-registry.mjs --json"
         "node scripts/check-v3-codex-boot-context-isomorphism.mjs --json"
         "node scripts/check-v3-context-pack-isomorphism.mjs --json"
         "node scripts/check-v3-memory-kb-isomorphism.mjs --json"
         "node scripts/check-v3-code-isomorphism-complete.mjs --dry-fixture --json"
       ])))
