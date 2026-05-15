(audit secret-store-cn-ecs-deploy
  :schema "missiond.work-order.audit.v1"
  :intent "20260513-secret-store-cn-ecs-deploy/intent.lisp"
  :plan "20260513-secret-store-cn-ecs-deploy/plan.lisp"
  :events
  ((event e1
     :at "2026-05-13T13:20:00+08:00"
     :actor "codex"
     :kind "work-order-created"
     :summary "Created a dedicated work-order for CN Secret Store deployment and Aliyun DNS governance.")
   (event e2
     :at "2026-05-13T13:21:00+08:00"
     :actor "codex"
     :kind "skill-evidence"
     :summary "Read deploy-ops, aliyun, and secret-store skills. The aliyun skill contains active ECS/runtime facts and inline credential-like SSH material that must be migrated to secret refs before reuse in worker prompts.")
   (event e3
     :at "2026-05-13T13:22:00+08:00"
     :actor "codex"
     :kind "runtime-fact"
     :summary "Confirmed current Secret Store SSOT says production is on the GCP xjp-backend VM; CN/ECS Secret Store remains a target requirement until deployed and recorded.")
   (event e4
     :at "2026-05-13T13:31:00+08:00"
     :actor "codex"
     :kind "aliyun-openapi-read"
     :summary "Aliyun OpenAPI with aliyun-global secret refs verified the ECS account and domain inventory. ECS instance i-uf6641fl52xo7ukf7kgl is Running in cn-shanghai-e with public IP 106.15.2.17. Domains visible: changtu.pro, pcea.top, xiaojinpro.com.")
   (event e5
     :at "2026-05-13T13:34:00+08:00"
     :actor "codex"
     :kind "deploy-center-read"
     :summary "Deploy Center lists ECS deploy-agent online at version 10.7.2 with projects jinstudio, pcea, and pceaapi. ECS container inventory currently has pcea-app, pcea-api, and pcea-postgres; no secret-store container.")
   (event e6
     :at "2026-05-13T13:35:00+08:00"
     :actor "codex"
     :kind "deploy-center-gap"
     :summary "Deploy Center project secret-store exists but has no deployment history and no agent execution config. CN Secret Store deployment is blocked until a CN runtime/project config, DB/KEK secret refs, domain, and rollback plan are declared.")
   (event e7
     :at "2026-05-13T14:27:00+08:00"
     :actor "codex"
     :kind "deploy-center-project-created"
     :summary "Created Deploy Center project secret-store-cn as a separate CN runtime shell: repo rickyjim626/secret-store-rs, branch main, docker_compose, target_host ecs-agent, target_path /opt/secret-store-cn, skip_deployment=true.")
   (event e8
     :at "2026-05-13T14:29:00+08:00"
     :actor "codex"
     :kind "deploy-center-agent-exec-config"
     :summary "Set secret-store-cn agent execution config to docker_compose with repo_url, branch main, work_dir /opt/secret-store-cn, compose_file docker/docker-compose.stg.yml, timeout 900s.")
   (event e9
     :at "2026-05-13T14:51:00+08:00"
     :actor "codex"
     :kind "deploy-center-stage-config"
     :summary "Corrected normalized build/deploy stage configs for secret-store-cn to executor_name=ecs-agent, executor_project=secret-store-cn, stage_project_slug=secret-store-cn, enabled=false. The stages are intentionally disabled until CN endpoint, DB/KEK/admin refs, and production compose override are declared.")
   (event e10
     :at "2026-05-13T14:52:00+08:00"
     :actor "codex"
     :kind "deploy-center-observability-gap"
     :summary "xjp project info secret-store-cn still reports No agent configured even though disabled normalized stage configs exist. Treat this as a read-model/UI gap; stage-configs API is the authority.")
   (event e11
     :at "2026-05-13T20:32:00+08:00"
     :actor "codex"
     :kind "aliyun-risk-control"
     :summary "Aliyun DNS OpenAPI from the local Mac triggered risk control because the observed source location was outside China. Future Aliyun DNS operations must use a domestic execution lane or user console approval, not this Mac as the caller.")
   (event e12
     :at "2026-05-13T20:40:00+08:00"
     :actor "codex"
     :kind "dns-fact"
     :summary "ss-cn.xiaojinpro.com is now the selected CN Secret Store hostname and resolves to Aliyun ECS 106.15.2.17. This is endpoint fact, not proof that the secret-store-cn service is deployed.")
   (event e13
     :at "2026-05-13T20:46:00+08:00"
     :actor "codex"
     :kind "deploy-lane-failure"
     :summary "GHCR image publish for secret-store-rs failed and is the wrong lane for ECS/CN anyway. ECS and Synology/domestic-only targets must not rely on target-side GitHub/GHCR; use cn-oss-bundle-lane.")
   (event e14
     :at "2026-05-13T21:02:00+08:00"
     :actor "codex"
     :kind "topology-fact"
     :summary "User provided topology: privatecloud Ubuntu 10900KF+6800XT, Windows 12900KF+3090Ti, and Synology VM are in the same LAN. privatecloud is the preferred CN builder/jump candidate when its deploy-agent is online. Current privatecloud deploy-agent check returned client offline/wake signal.")
   (event e15
     :at "2026-05-15T22:13:39+08:00"
     :actor "codex"
     :kind "runtime-smoke"
     :summary "Verified https://ss-cn.xiaojinpro.com/livez and /readyz both return 200 OK. Treat secret-store-cn as running on Aliyun ECS; deploy-center still has a stale runtime shell with skip_deployment/stage configs disabled and incomplete release provenance.")
   (event e16
     :at "2026-05-15T22:13:42+08:00"
     :actor "codex"
     :kind "adjacent-runtime-smoke"
     :summary "Verified changtu.pro currently returns Aliyun ICP interstitial for public host requests; direct ECS/IP request reaches Nginx/PCEA default page. Long Image CN remains deployed behind host-header evidence but not publicly launched until ICP or alternative edge/domain strategy is complete."))
  :current_status "runtime-verified-with-provenance-gap"
  :next_action "Promote the ad-hoc GCP docker-save -> Aliyun OSS -> ECS docker-load Secret Store CN deployment into deploy-center release provenance, then replace the stale skip_deployment/stage-disabled read model with a configured CN OSS artifact lane.")
