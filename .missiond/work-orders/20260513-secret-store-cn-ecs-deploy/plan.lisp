(plan secret-store-cn-ecs-deploy
  :schema "missiond.work-order.plan.v1"
  :intent "20260513-secret-store-cn-ecs-deploy/intent.lisp"
  :status "blocked"
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
      (step s2 :status partial :action "Verified current ECS container inventory; no secret-store container is running, and a dedicated CN DB/KEK plan is still missing.")
      (step s3 :status partial :action "Verified Aliyun domains and changtu.pro records; chosen Secret Store CN hostname is still undeclared.")
      (step s4 :status done :action "Verified Deploy Center project shape for existing ECS deployments and confirmed secret-store lacks agent execution config.")))
   (phase p3
     :name "deploy-and-dns"
     :status "blocked"
     :steps
     ((step s0 :status done :action "Created Deploy Center project secret-store-cn with skip_deployment=true, target_host=ecs-agent, work_dir=/opt/secret-store-cn, and docker_compose agent execution config.")
      (step s0b :status done :action "Corrected normalized build/deploy stage configs for secret-store-cn to executor_name=ecs-agent, executor_project=secret-store-cn, stage_project_slug=secret-store-cn, enabled=false.")
      (step s1 :status pending :action "Deploy secret-store-rs image/container to the CN runtime target with independent DB/KEK material.")
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
     :kind "runtime-contract-disabled"
     :summary "Deploy Center now has secret-store-cn project and disabled ECS build/deploy stage configs; deployment remains blocked until endpoint, runtime material, and production compose override are declared.")
   (block b2
     :kind "missing-endpoint-decision"
     :summary "CN Secret Store endpoint/domain is not yet declared; DNS mutation would be premature.")
   (block b3
     :kind "missing-db-kek-plan"
     :summary "Independent CN DB/KEK/admin-key material is not yet declared as secret refs.")
   (block b4
     :kind "deploy-center-read-model-gap"
     :summary "xjp project info still says No agent configured when only disabled normalized stage configs exist; deploy-center should show configured-disabled to avoid operator confusion.")))
