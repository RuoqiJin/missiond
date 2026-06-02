(work-order-intent
  :schema "missiond.work-order.intent.v1"
  :id "wo-20260602204120-unscoped-evidence-read-model-guard"
  :title "Guard unscoped context-gather evidence read model"
  :objective "Prevent mission_context_gather conversation_audit or default unscoped queries from pulling project-scoped persisted evidence_items when no project was resolved."
  :evidence ["Live conversation_audit query without project pulled Payments and ASR support_refs from evidence_items even though the query was intended as unscoped historical audit."]
  :write-scope [".missiond/v3/shards/request-runtime.lisp"
                "crates/missiond-daemon/src/handlers/knowledge/context_gather.rs"
                "scripts/check-v3-memory-kb-isomorphism.mjs"
                "generated-v3-contracts"])
