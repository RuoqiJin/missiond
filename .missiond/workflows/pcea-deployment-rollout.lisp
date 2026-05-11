(workflow pcea-deployment-rollout
  :schema "missiond.workflow.v1"
  :workflow_id pcea-deployment-rollout
  :status active
  :source_plans [m6-deployment-rollout deployment-event-response skill-runtime pcea-deploy-skill-investigation]
  :match_rules
    ((trigger :kind manual :tool mission_swarm_run :when "objective asks to deploy or verify PCEA frontend/backend")
     (trigger :kind boardtask :title-prefix "Deploy PCEA")
     (dedupe-key "pcea-deployment-rollout:<component>:<target_commit>"))
  :owner missiond
  :purpose "Deploy and verify the two-component PCEA production system through deploy-center while using skill registry context as operator evidence, not as hidden prompt preload."
  :inputs [BoardTask service-runtime-universe deploy-center.provenance missiond.event-envelope.v1 skill-runtime.context pcea.repos]
  :entry ["mission_skill_context(resolve,skill=pcea,include_kb=false)" "mission_infra_query(action=skill_evidence,target=pcea)" "scripts/check-m6-deployment-status.mjs --json" "deploy-center provenance endpoint" "deployment-event-response"]
  :components
    ((component pcea-video-vault
       :repo "/Users/jinchen/Downloads/PCEA develop/pcea-video-vault"
       :branch main
       :deploy_slug pcea-video-vault
       :agent_project PCEA
       :owns [frontend-image docker-compose deploy-sh deploy-api-sh sql-bundle]
       :artifact "OSS bundle: image.tar.gz + docker-compose.yml + deploy.sh + deploy-api.sh + sql/"
       :runtime_service pcea
       :port 3001
       :health "/")
     (component pcea-api
       :repo "/Users/jinchen/Downloads/PCEA develop/pcea-api"
       :branch master
       :deploy_slug pcea-api
       :agent_project PCEAAPI
       :owns [backend-image service.manifest.toml]
       :artifact "OSS image bundle only; runtime image tag pcea-api:latest is local on ECS"
       :runtime_service pcea-api
       :port 3002
       :health "/api/health"))
  :steps
    ((step s1 :name review-question
       :logic "Ask: what PCEA component is being deployed, what commit is intended, and what evidence is needed to prove deploy-center already knows the production state?")
     (step s2 :name resolve-skill-context
       :logic "Call skill-runtime context for pcea with include_kb=false; include required deployment/deploy-ops/deployment-troubleshoot skill dependencies as evidence refs, not prompt preloads.")
     (step s3 :name reconcile-deployment-evidence
       :logic "Reconcile PCEA deployment SSOT, pcea skill evidence, deploy-center project/provenance, ECS runtime target facts, and current GCP-hosted Secret Store health for ecs-agent claim auth before running any deploy; if ECS/agent/script/secret-store facts disagree, block with a drift report.")
     (step s4 :name build-evidence-plan
       :logic "Collect component repo HEAD, default branch, deploy slug, deploy-center provenance, previous release, rollback evidence, and expected smoke endpoints.")
     (step s5 :name classify-component-order
       :logic "If both components are targeted, deploy pcea-video-vault before pcea-api because video-vault owns shared docker-compose.yml, deploy.sh, deploy-api.sh, and sql/ bundle files.")
     (step s6 :name dispatch-deploy-ops-shard
       :logic "Create a deploy-ops worker shard with exact component, repo, branch, target commit, deploy slug, previous provenance, smoke list, rollback evidence, and no DNS/secret mutation permission.")
     (step s7 :name wait-deploy-center-events
       :logic "Wait through EventBridge for deploy_started, deploy_succeeded, smoke_succeeded, deploy_failed, or smoke_failed using project_id=pcea, service_id=deploy-center, deploy slug, and correlation_id.")
     (step s8 :name smoke-components
       :logic "For successful deploys verify frontend / on port 3001 and backend /api/health on port 3002; deeper pcea-api smoke may read service.manifest.toml probes when deploy-center exposes them.")
     (step s9 :name write-rollout-report
       :logic "Record deployed component, target commit, deploy-center provenance URL, events observed, smoke result, rollback artifact, and any skill/runtime drift."))
  :risk-gates
    ((gate g1 :rule "Deployment mutation must go through deploy-center/deploy-ops; MissionD does not run ad-hoc production deploy commands.")
     (gate g2 :rule "No Cloudflare, DNS, secret, or production env mutation from this workflow.")
     (gate g3 :rule "PCEA deployment state is answered by deploy-center provenance and relayed deployment events; curl/git probes are diagnostics only.")
     (gate g4 :rule "pcea-api image delivery is OSS bundle/local image tag; do not assume GHCR fallback exists for the backend.")
     (gate g5 :rule "pcea-video-vault owns shared compose and deploy scripts; backend deploy must not overwrite those shared files.")
     (gate g6 :rule "PCEA deploy workers must read PCEA skill evidence and deployment SSOT before acting, because ECS path, OSS bundle, jump-host, and deploy-agent details are not safely inferable from repo names.")
     (gate g7 :rule "If ecs-agent is calling claim-next but Secret Store cannot resolve ecs-agent/DEPLOY_AGENT_API_KEY from the GCP xjp-backend VM Secret Store runtime, classify PCEA deployment as deploy-blocked-by-secret-store; do not misclassify it as PCEA code, GitHub Actions, ECS script failure, or historical ClawCloud outage."))
  :completion
    ((criterion c1 :rule "Each targeted component has deploy-center provenance for the requested commit or an explicit blocked reason.")
     (criterion c2 :rule "Each targeted component has durable deploy-center event evidence and smoke evidence.")
     (criterion c3 :rule "The rollout report captures any skill drift, manifest gap, migration/manual step gap, Secret Store claim-auth dependency failure, or deploy-agent evidence gap as follow-up work.")))
