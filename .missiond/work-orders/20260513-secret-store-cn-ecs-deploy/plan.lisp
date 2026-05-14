(plan secret-store-cn-ecs-deploy
  :schema "missiond.work-order.plan.v1"
  :intent "20260513-secret-store-cn-ecs-deploy/intent.lisp"
  :status "blocked-on-domestic-artifact-lane"
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
     :status "blocked"
     :steps
     ((step s1 :status done :action "Verified ECS reachability through the deploy-agent lane.")
      (step s2 :status partial :action "Verified current ECS container inventory; no secret-store container is running. CN DB/KEK/admin refs are declared externally but must remain credential refs only.")
      (step s3 :status done :action "Declared CN Secret Store hostname ss-cn.xiaojinpro.com; DNS currently resolves to Aliyun ECS 106.15.2.17.")
      (step s4 :status done :action "Verified Deploy Center project shape for existing ECS deployments and confirmed secret-store lacks agent execution config.")))
   (phase p3
     :name "deploy-and-dns"
     :status "blocked"
     :steps
     ((step s0 :status done :action "Created Deploy Center project secret-store-cn with skip_deployment=true, target_host=ecs-agent, work_dir=/opt/secret-store-cn, and docker_compose agent execution config.")
      (step s0b :status done :action "Corrected normalized build/deploy stage configs for secret-store-cn to executor_name=ecs-agent, executor_project=secret-store-cn, stage_project_slug=secret-store-cn, enabled=false.")
      (step s1 :status blocked :action "Deploy secret-store-rs to CN through cn-oss-bundle-lane, not GHCR/GitHub target pull. GHCR workflow failed and ECS/CN target cannot be treated as registry-capable.")
      (step s2 :status pending :action "Configure reverse proxy and health checks.")
      (step s3 :status pending :action "Apply Aliyun DNS only after preflight and rollback manifest are complete.")
      (step s4 :status pending :action "Run /livez, /readyz, namespace listing, and encryption/decryption smoke.")))
   (phase p4
     :name "evidence-promotion"
     :status "pending"
     :steps
     ((step s1 :status pending :action "Write deploy-center runtime/provenance evidence.")
      (step s2 :status pending :action "Update MissionD infrastructure-universe evidence refs.")
      (step s3 :status pending :action "Record credential migration report for skill inline secrets.")))))
  :acceptance
  ((check c1 "CN endpoint health returns 200.")
   (check c2 "Secret Store namespace list works against CN runtime with a scoped credential.")
   (check c3 "MissionD/deploy-center can answer where secret-store-cn is deployed and which secret refs it uses.")
   (check c4 "No credential value is committed or printed in work-order artifacts.")
   (check c5 "Rollback steps for DNS/runtime are recorded."))
  :blocked_by
  ((block b1
     :kind "domestic-artifact-lane-required"
     :summary "ECS/CN cannot depend on target-side GitHub/GHCR. Deploy secret-store-cn through cn-oss-bundle-lane: approved builder -> Aliyun OSS -> ECS internal download -> deploy-agent run -> reported digest.")
   (block b2
     :kind "build-runtime-offline"
     :summary "privatecloud deploy-agent currently reports client offline. It is the preferred CN builder/jump candidate because privatecloud, Windows 12900kf, and Synology VM share the xjp-zibo-lan.")
   (block b3
     :kind "deploy-center-read-model-gap"
     :summary "xjp project info still says No agent configured when only disabled normalized stage configs exist; deploy-center should show configured-disabled and selected artifact lane to avoid operator confusion.")
   (block b4
     :kind "provenance-contract-required"
     :summary "The next deploy must record source commit, builder id, OSS object key, artifact sha256, reported digest, smoke result, and rollback bundle.")))
