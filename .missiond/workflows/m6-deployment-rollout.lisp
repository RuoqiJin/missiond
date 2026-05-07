(workflow m6-deployment-rollout
  :schema "missiond.workflow.v1"
  :workflow_id m6-deployment-rollout
  :status active
  :source_plans [project-maturity-registry service-runtime-universe deployment-event-response]
  :match_rules
    ((trigger :kind manual :tool mission_swarm_run :when "objective asks which M6 projects are not deployed or asks to deploy M6 services")
     (trigger :kind boardtask :title-prefix "Deploy M6 services")
     (dedupe-key "m6-deployment-rollout:<project_id>:<target_commit>"))
  :owner missiond
  :purpose "Close the gap between Auth-grade M6 SSOT maturity and production deployment evidence without making MissionD the deployment fact authority."
  :inputs [project-maturity-registry service-runtime-universe deploy-center.status deploy-center.provenance missiond.event-envelope.v1 BoardTask]
  :entry ["scripts/check-m6-deployment-status.mjs --json" "deploy-center /api/deploy/status" "deploy-center provenance endpoint" "deployment-event-response"]
  :steps
    ((step s1 :name read-m6-maturity
       :logic "Read MissionD project-maturity-registry and select projects whose current maturity is M6.")
     (step s2 :name query-deploy-center
       :logic "Query deploy-center status/provenance for each M6 project's deployment slug; do not infer release state from curl/git when deploy-center has a durable answer.")
     (step s3 :name compare-local-service-delta
       :logic "For git-backed services compare the latest successful deploy commit to local HEAD across that service's SSOT/runtime paths; classify deployed-current, deployed-stale, not-confirmed, or deployed-unknown.")
     (step s4 :name order-rollout
       :logic "Deploy deployment infrastructure before dependents: deploy-center first, then router, then pcea; auth is skipped unless auth-relevant files changed after its successful deploy.")
     (step s5 :name deploy-through-deploy-center
       :logic "Create deploy-ops BoardTask shards that use deploy-center / deploy-ops capability; MissionD supervises and waits for deploy-center events rather than running ad-hoc deploy commands.")
     (step s6 :name smoke-and-observe
       :logic "After each deploy wait for deploy_succeeded and smoke_succeeded events, then run service-specific compatibility smoke and record evidence.")
     (step s7 :name write-rollout-report
       :logic "Write a rollout report listing deployed commit, smoke result, rollback artifact, and remaining deployment fact gaps."))
  :risk-gates
    ((gate g1 :rule "No Cloudflare/DNS/secret mutation from this workflow.")
     (gate g2 :rule "MissionD does not auto rollback; rollback requires deploy-center policy or explicit approval.")
     (gate g3 :rule "deploy-center remains deployment fact authority; MissionD may cache and display but not override release provenance.")
     (gate g4 :rule "M6 maturity is not deployment evidence; a project marked M6 but lacking deploy-center confirmation is deployment-not-confirmed, not assumed offline."))
  :completion
    ((criterion c1 :rule "scripts/check-m6-deployment-status.mjs reports every M6 project as deployed-current or explicitly no-deploy-target.")
     (criterion c2 :rule "deploy-center status/provenance is available for every deployed M6 service slug.")
     (criterion c3 :rule "Each deployment task closes only after durable deploy-center event evidence and smoke evidence.")))
