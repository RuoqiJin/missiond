(work-order-intent
  :schema "missiond.work-order.intent.v1"
  :id "wo-20260603002925-Distinguish-Deploy-Center-webhoo"
  :objective "Distinguish Deploy Center webhook probes from authoritative relay evidence in mission_context_gather"
  :source external-codex
  :status accepted
  :unknowns ["Production Deploy Center still must prove durable deploy_events emission separately from MissionD webhook probe ingestion."]
  :evidence_refs ["mission_timeline:system::external_service_event seq=12939 local_webhook_probe" "mission_context_gather deploy_ops observed manual_probe as non-authoritative probe"]
  :constraints ["Lisp-first" "no-secret-values" "commit-through-work-order-gate"])
