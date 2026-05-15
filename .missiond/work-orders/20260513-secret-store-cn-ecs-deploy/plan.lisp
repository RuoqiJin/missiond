(plan secret-store-cn-ecs-deploy
  :schema "missiond.work-order.plan.v1"
  :intent "20260513-secret-store-cn-ecs-deploy/intent.lisp"
  :status "runtime-verified-with-provenance-gap"
  :owner "codex"
  :preferred_worker_lane "claude-code-deploy-ops"
  :phases
  ((phase p1
     :name "evidence-collection"
     :status "done"
     :steps
     ((step s1 :status done
        :action "Read deploy-ops, aliyun, and secret-store skills.")
      (step s2 :status done
        :action "Read secret-store and deploy-center SSOT for runtime target and deploy-agent facts.")
      (step s3 :status done
        :action "Query deploy-center/agent status for the Aliyun ECS lane without mutating production state.")))
   (phase p2
     :name "cn-runtime-preflight"
     :status "done"
     :steps
     ((step s1 :status done :action "Verified ECS reachability through the deploy-agent lane.")
      (step s2 :status done :action "Verified CN Secret Store runtime externally: /livez and /readyz return 200 over https://ss-cn.xiaojinpro.com. CN DB/KEK/admin refs remain credential refs only.")
      (step s3 :status done :action "Declared CN Secret Store hostname ss-cn.xiaojinpro.com; DNS currently resolves to Aliyun ECS 106.15.2.17.")
      (step s4 :status done :action "Verified Deploy Center project shape for existing ECS deployments and confirmed secret-store lacks agent execution config.")))
   (phase p3
     :name "deploy-and-dns"
     :status "partial"
     :steps
     ((step s0 :status done :action "Created Deploy Center project secret-store-cn with skip_deployment=true, target_host=ecs-agent, work_dir=/opt/secret-store-cn, and docker_compose agent execution config.")
      (step s0b :status done :action "Corrected normalized build/deploy stage configs for secret-store-cn to executor_name=ecs-agent, executor_project=secret-store-cn, stage_project_slug=secret-store-cn, enabled=false.")
      (step s1 :status done :action "Secret Store CN was deployed through an ad-hoc domestic artifact path: GCP docker save -> Aliyun OSS -> ECS docker load. This must be promoted into first-class deploy-center provenance before the lane is considered complete.")
      (step s2 :status done :action "Reverse proxy is active: Nginx terminates https://ss-cn.xiaojinpro.com and proxies to 127.0.0.1:8091.")
      (step s3 :status done :action "Aliyun DNS points ss-cn.xiaojinpro.com to ECS 106.15.2.17.")
      (step s4 :status partial :action "Run /livez and /readyz smoke succeeded. Namespace and encryption/decryption smoke still require scoped credential execution through xjp CLI or deploy-ops lane.")))
   (phase p4
     :name "evidence-promotion"
     :status "partial"
     :steps
     ((step s1 :status partial :action "Write deploy-center runtime/provenance evidence. Current deploy-center project still reports skip_deployment/stage disabled and has no recorded deployed commit.")
      (step s2 :status done :action "Update MissionD infrastructure-universe evidence refs to mark secret-store-cn runtime verified with provenance gap.")
      (step s3 :status pending :action "Record credential migration report for skill inline secrets."))))
  :acceptance
  ((check c1 "CN endpoint health returns 200.")
   (check c2 "Secret Store namespace list works against CN runtime with a scoped credential.")
   (check c3 "MissionD/deploy-center can answer where secret-store-cn is deployed and which secret refs it uses.")
   (check c4 "No credential value is committed or printed in work-order artifacts.")
   (check c5 "Rollback steps for DNS/runtime are recorded."))
  :open_gaps
  ((gap g1
     :kind "deploy-center-read-model-gap"
     :summary "xjp project info still says No agent configured when only disabled normalized stage configs exist; deploy-center should show configured-disabled and selected artifact lane to avoid operator confusion.")
   (gap g2
     :kind "provenance-contract-required"
     :summary "The ad-hoc CN deployment must be backfilled with source commit, builder id, OSS object key, artifact sha256, reported digest, smoke result, and rollback bundle.")
   (gap g3
     :kind "docker-healthcheck-disabled"
     :summary "The deployed LTS image lacks current source health-check early-return behavior; external HTTP smoke is authority until the next image promotion.")))
