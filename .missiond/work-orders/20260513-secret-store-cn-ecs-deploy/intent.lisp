(intent secret-store-cn-ecs-deploy
  :schema "missiond.work-order.intent.v1"
  :created_at "2026-05-13T13:20:00+08:00"
  :created_by "codex"
  :source "user-request"
  :objective "Bring up an independent China-region Secret Store runtime on Aliyun ECS and make its deployment facts auditable through MissionD/deploy-center."
  :project_id "secret-store"
  :partition "cn"
  :canonical_service "secret-store-rs"
  :known_facts
  ((fact f1 :status verified
     :text "Global/current production Secret Store is https://ss.xiaojinpro.top on the GCP xjp-backend VM.")
   (fact f2 :status verified
     :text "The new Aliyun global AccessKey is stored in Secret Store namespace aliyun-global as secret refs only.")
   (fact f3 :status verified
     :text "Aliyun DNS read access for changtu.pro has been verified with the new aliyun-global credential.")
   (fact f4 :status evidence
     :text "Aliyun ECS runtime facts are available from the aliyun skill, but credential material in skills must be migrated to secret refs before worker context reuse.")
   (fact f5 :status pending
     :text "A CN Secret Store runtime is a target requirement, not yet a verified deploy-center runtime fact."))
  :required_questions
  ((question q1 "Which domain should the CN Secret Store use: reuse ss.xiaojinpro.top with regional routing, or create a CN-specific host such as ss-cn.xiaojinpro.top?")
   (question q2 "Does the CN Secret Store get an independent Postgres database and KEK on Aliyun ECS, or a temporary same-VM/container DB for bootstrap?")
   (question q3 "Should the deploy-center record this as a separate runtime target before DNS mutation?"))
  :constraints
  ((constraint c1 "Do not print or write cloud AccessKey values, SSH passwords, API keys, KEK material, or admin keys into Lisp, Board notes, prompts, or logs.")
   (constraint c2 "All secrets must be referenced as secret-store:// namespace/key refs.")
   (constraint c3 "DNS mutation must have explicit target record, target IP/CNAME, rollback record, and post-change health check.")
   (constraint c4 "Deployment facts must be promoted into deploy-center/MissionD Universe evidence after verification.")
   (constraint c5 "Use deploy-agent/deploy-ops path where available; raw SSH is fallback evidence gathering only."))
  :desired_result
  ((result r1 "CN Secret Store runtime reachable from an agreed CN endpoint.")
   (result r2 "Deploy-center and MissionD Universe know the runtime target, service, health endpoint, and credential refs.")
   (result r3 "Aliyun DNS operation and CN deployment steps are recorded as replayable work-order audit evidence.")
   (result r4 "Inline credential facts in skills are flagged for migration to secret refs.")))
