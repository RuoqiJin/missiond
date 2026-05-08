(workflow deployment-event-response
  :schema "missiond.workflow.v1"
  :workflow_id deployment-event-response
  :status active
  :source_plans [eventbridge-policy deployment-event-ingest deploy-agent-self-update-governance]
  :match_rules
    ((trigger :kind event :event SystemEvent.ExternalServiceEvent :service_id deploy-center)
     (trigger :kind webhook :path "/webhooks/deploy-center-event")
     (dedupe-key "deployment-event-response:<service_id>:<event_id>"))
  :owner missiond
  :purpose "Respond to deploy-center deployment events relayed into MissionD EventBridge without making MissionD the deployment fact authority."
  :inputs [SystemEvent.ExternalServiceEvent missiond.event-envelope.v1 deploy-center.provenance BoardTask]
  :entry ["/webhooks/deploy-center-event" "mission_timeline(action=wait,domain=system)" "resident-master-control"]
  :steps
    ((step s1 :name observe-deployment-event
       :logic "Accept deploy-center event envelope only after token/idempotency validation; PTY and curl probes are diagnostic fallback, not authority.")
     (step s2 :name classify-risk
       :logic "Classify deploy_failed, smoke_failed, rollback_failed, and agent_update_failed as actionable; classify succeeded/provenance_changed/heartbeat as observe-only unless a waiting BoardTask requested them. Classify provenance_partial, reported_digest_missing, and digest_resolution_failed as evidence-quality gaps: they may not block a healthy deployment, but they must create or update a deploy-agent/deploy-center hardening task.")
     (step s3 :name attach-authority
       :logic "Attach deploy-center provenance URL, deploy_event_id, session_id, project_id, correlation_id, and service_id to the context pack.")
     (step s4 :name create-deploy-ops-task
       :logic "For actionable events create a visible deploy-ops BoardTask with evidence and suggested next checks; do not auto deploy, rollback, mutate DNS, or change secrets.")
     (step s5 :name wait-or-close
       :logic "If the event completes a waiting deploy task, close only after durable event evidence, deploy-center provenance, and smoke evidence agree; otherwise leave a diagnostic note. Deploy-center notify HTTP 200 and GitHub Actions success are progress evidence, not closure evidence."))
  :risk-gates
    ((gate g1 :id no-auto-rollback :rule "MissionD must not auto rollback, auto deploy, mutate DNS, or mutate secrets from an external deployment event.")
     (gate g2 :rule "Mutating deployment actions require deploy-center policy or explicit Board/user approval.")
     (gate g3 :rule "deploy-center provenance remains the release authority; MissionD event logs are cache, visibility, and workflow triggers.")
     (gate g4 :rule "Secrets, Cloudflare credentials, and deploy tokens stay outside Lisp and are never inferred from event payloads.")
     (gate g5 :rule "Partial provenance is surfaced as follow-up work, not silently upgraded to full release confidence."))
  :completion
    ((criterion c1 :rule "Every actionable deploy failure produces a visible deploy-ops BoardTask or attaches evidence to an existing waiting deployment task.")
     (criterion c2 :rule "Observe-only deployment events are stored and become waitable by service_id, event_kind, project_id, and correlation_id.")
     (criterion c3 :rule "No deployment mutation is executed directly by this workflow.")))
