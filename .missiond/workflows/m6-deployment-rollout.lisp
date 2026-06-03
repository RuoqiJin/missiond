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
  :inputs [project-maturity-registry service-runtime-universe deploy-center.status deploy-center.provenance deploy-center.workflow-run missiond.event-envelope.v1 BoardTask mission_infra_query.skill_evidence project-deployment-ssot]
  :entry ["scripts/check-m6-deployment-status.mjs --json" "mission_infra_query(action=skill_evidence|reconcile)" "deploy-center /api/deploy/status" "deploy-center provenance endpoint" "deployment-event-response" "GitHub Actions workflow observation as diagnostic-only evidence"]
  :steps
    ((step s1 :name read-m6-maturity
       :logic "Read MissionD project-maturity-registry and select projects whose current maturity is M6.")
     (step s2 :name query-deploy-center
       :logic "Query deploy-center status/provenance for each M6 project's deployment slug; do not infer release state from curl/git when deploy-center has a durable answer.")
     (step s3 :name compare-local-service-delta
       :logic "For git-backed services compare the latest successful deploy commit to local HEAD across that service's SSOT/runtime paths; classify deployed-current, deployed-stale, not-confirmed, or deployed-unknown.")
     (step s4 :name preflight-deployment-substrate
       :logic "Before dispatching a deploy, record deploy-center health, previous provenance, GitHub workflow/runner status, rollback artifact source, and known cache accelerators. sccache/cache acceleration failure is diagnostic and must not block Docker/service build if the build has a fallback.")
    (step s5 :name resolve-project-deployment-evidence
      :logic "Before attempting deploy or choosing host/script/agent paths, read the project's deployment SSOT, deploy-center runtime facts, Secret Store dependency health for executor claim auth, and skill-derived operational evidence through mission_infra_query; if the facts disagree, create a drift diagnostic instead of guessing.")
    (step s5b :name verify-executor-claim-dependencies
      :logic "For deploy-center pull-mode executors, verify that deploy_executors.api_key_ref -> Secret Store DEPLOY_AGENT_API_KEY is reachable from the current GCP xjp-backend VM runtime before concluding an agent is offline. If Secret Store is unavailable, classify deploy-blocked-by-secret-store against gcp-runtime/Caddy/docker/xjp-postgres health and do not keep re-dispatching deploy workers.")
     (step s5c :name resolve-break-glass-runbook
       :logic "If deploy-center reports agent_offline, repeated agent_update_failed, or unreachable provenance for an otherwise known runtime target, query mission_infra_query(action=skill_evidence|credential_refs) for break_glass_runbook_refs and produce an approval-gated manual-ops option. Manual ECS/SSH/OSS/deploy.sh guidance from skills may enter the context pack as redacted evidence, but deploy-center remains the authority to reconcile the final deployment fact.")
     (step s5d :name select-artifact-delivery-lane
       :logic "Before any build or deploy worker is dispatched, ask deploy-center for target-network-profile and artifact-delivery-lane. CN restricted targets such as Aliyun ECS/Synology must use cn-oss-bundle-lane or an approved domestic mirror+builder lane; GHCR/GitHub target pulls and target-side source builds are invalid architecture, not retryable transient failures.")
     (step s5e :name select-source-sync-lane
       :logic "For managed build targets such as rickyhq Mac mini, sync source through GitHub or XJP codebase/deploy-center CodebaseSyncOperation, then run build/test/install on the target node. Operator-laptop rsync/scp is break-glass only, must be recorded as a failed-process diagnostic if attempted, and must not be used as the steady-state path.")
     (step s6 :name order-rollout
       :logic "Deploy deployment infrastructure before dependents: deploy-center first, then router, then pcea; auth is skipped unless auth-relevant files changed after its successful deploy.")
     (step s7 :name deploy-through-deploy-center
       :logic "Create deploy-ops BoardTask shards with context_intent=deploy-ops, task_class=deploy-ops, pool_hint=claude-code-deploy-ops, engine_hint=claude-code, and structured smoke commands. MissionD supervises and waits for deploy-center events rather than running ad-hoc deploy commands. GitHub workflow success and deploy-center notify HTTP 200 only move the task into wait-for-provenance; they are not completion evidence. Waiting for GitHub Actions/build completion must use deploy-center events/provenance or bounded XJP MCP tools such as xjp_build_wait/xjp_deploy_watch; naked gh api polling loops are forbidden.")
     (step s8 :name smoke-and-observe
       :logic "After each deploy, wait through EventBus for deploy-center closure_verdict with verdict=success before treating the release as closed. deploy_succeeded and smoke_succeeded, GitHub workflow success, and curl probes are intermediate evidence only; if structured smoke commands run outside deploy-center, record them as ReleaseEvidence inputs before closure. Shell sleep loops are forbidden; naked gh api polling loops are forbidden; scheduled monitor/wakeup, xjp_build_wait/xjp_deploy_watch, or deploy-center polling surfaces own waiting.")
     (step s9 :name classify-provenance-diagnostics
       :logic "Classify digest_resolution_failed, reported_digest_missing, runner_queued, build_cache_unavailable, and provenance_partial separately from deploy_failed. A deployed-current service with reported_digest_missing remains operationally current but carries a deploy-agent provenance gap.")
     (step s10 :name write-rollout-report
       :logic "Write a rollout report listing deployed commit, smoke result, rollback artifact, deploy-center provenance confidence, and remaining deployment fact gaps."))
  :risk-gates
    ((gate g1 :rule "No Cloudflare/DNS/secret mutation from this workflow.")
     (gate g2 :rule "MissionD does not auto rollback; rollback requires deploy-center policy or explicit approval.")
     (gate g3 :rule "deploy-center remains deployment fact authority; MissionD may cache and display but not override release provenance.")
     (gate g4 :rule "M6 maturity is not deployment evidence; a project marked M6 but lacking deploy-center confirmation is deployment-not-confirmed, not assumed offline.")
     (gate g5 :rule "CI/build/push success, GitHub Actions green, and deploy-center notify HTTP 200 are insufficient to close a deployment BoardTask without deploy-center provenance plus smoke evidence.")
     (gate g6 :rule "Build cache accelerators such as sccache/kellnr are performance aids; unavailable cache infrastructure must either fall back cleanly or produce a diagnostic gap, not become an implicit release blocker.")
     (gate g7 :rule "Deployment workers must consult project deployment SSOT plus mission_infra_query skill evidence before acting on any host/script/agent path; skill facts are evidence, deploy-center provenance is authority.")
     (gate g8 :rule "Pull-mode executor status must distinguish agent-offline from claim-auth-dependency-failed. Secret Store lookup failure for deploy_executors.api_key_ref / DEPLOY_AGENT_API_KEY blocks deployment and must surface as deploy-blocked-by-secret-store on gcp-runtime, not as a PCEA, deploy-agent, or historical ClawCloud bug.")
     (gate g9 :rule "Agent-offline manual fallback is break-glass only: the context pack may include redacted skill runbook refs and credential refs, but manual SSH/ECS/deploy actions need explicit approval and must write post-action provenance evidence.")
     (gate g10 :rule "Domestic runtime targets cannot depend on target-side GitHub/GHCR/DockerHub availability. Deployment workers must use deploy-center artifact-delivery-lane selection and record lane provenance before attempting CN deploys.")
     (gate g11 :rule "Managed remote build nodes must receive source through GitHub or XJP codebase/deploy-center sync and build locally; direct rsync/scp source mirroring from an operator laptop is forbidden outside documented break-glass recovery."))
  :completion
    ((criterion c1 :rule "scripts/check-m6-deployment-status.mjs reports every M6 project as deployed-current or explicitly no-deploy-target.")
     (criterion c2 :rule "deploy-center status/provenance is available for every deployed M6 service slug.")
     (criterion c3 :rule "Each deployment task closes only after durable deploy-center event evidence and smoke evidence.")
     (criterion c4 :rule "Partial provenance such as reported_digest_missing is recorded as a follow-up deploy-agent/deploy-center evidence gap instead of being lost in the deployment summary.")
     (criterion c5 :rule "If deployment is blocked by Secret Store, rollout report names Secret Store as the failed dependency and records the executor namespace/key reference without exposing credential values.")
     (criterion c6 :rule "If deployment is blocked by agent_offline, rollout report names the runtime target, last heartbeat/update evidence, break-glass runbook refs, and approval boundary.")
     (criterion c7 :rule "For CN targets, rollout report names the selected artifact-delivery-lane, builder/runtime target, transfer store, artifact digest, and why GHCR/GitHub target pulls were not used.")
     (criterion c8 :rule "For Mac mini / managed local-build targets, rollout report names the source-sync lane (GitHub or XJP codebase), target-side build command, release id, smoke result, and rollback artifact.")))
