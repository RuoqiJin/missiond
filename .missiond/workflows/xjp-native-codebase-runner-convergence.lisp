(workflow xjp-native-codebase-runner-convergence
  :schema "missiond.workflow.v1"
  :workflow_id xjp-native-codebase-runner-convergence
  :status active
  :source_plans [deploy-center-ssot infrastructure-universe m6-deployment-rollout]
  :match_rules
    ((trigger :kind boardtask :title-prefix "Build XJP native codebase runner")
     (trigger :kind manual :tool mission_swarm_run :when "objective asks to replace GitHub/GitHub Actions with XJP-owned codebase/workflow/runner")
     (dedupe-key "xjp-native-codebase-runner:<project_id>:<phase>"))
  :owner missiond
  :purpose "Build an XJP-owned GitHub/GitHub-Actions replacement without losing deploy-center provenance, MissionD orchestration, domestic artifact lanes, or Forge component reuse."
  :inputs [deploy-center.codebase-runner-blueprint deploy-center.runtime-target-inventory deploy-center.provenance MissionD.EventBridge MissionD.BoardTask Forge.catalog skill-evidence]
  :entry ["deploy-center .missiond/backend/dc-codebase-runner-blueprint.lisp" "deploy-center migrations/0042_xjp_native_codebase_workflow_runner.sql" "deploy-center migrations/0043_xjp_codebase_sync_operations.sql" "mission_swarm_run objective"]
  :steps
    ((step s1 :name review-question
       :logic "Ask: which GitHub/GitHub Actions responsibilities are truly missing for this project, and which are already covered by deploy-center/code-center/runner surfaces?")
     (step s2 :name evidence-plan
       :logic "Collect codebase, workflow, runner, artifact, provenance, domestic network, secret-ref, and Forge reuse evidence before proposing implementation shards.")
     (step s3 :name normalize-codebase-facts
       :logic "Map GitHub, Gitee, local upload, Forge import, and Code Center facts into XJP CodebaseRepository / CommitFact / RefFact; mark provider facts as compatibility or mirror facts.")
     (step s4 :name create-codebase-sync-operation
       :logic "When the task needs GitHub-like pull/push/fetch/mirror, create a CodebaseSyncOperation first. Default transport is deploy-agent-tunnel; ad-hoc rsync/ssh from an operator laptop is break-glass only and must not be the steady-state sync path.")
     (step s4b :name bind-sync-operation-to-runner
       :logic "Runnable CodebaseSyncOperation rows must create native WorkflowRun/WorkflowJob records, bind workflow_run_id/workflow_job_id back to the operation, and execute through deploy-agent metadata.codebase_sync rather than unstructured shell.")
     (step s5 :name compile-workflow-definition
       :logic "Convert imported GitHub Actions YAML, project SSOT, or deploy-center UI definitions into WorkflowDefinition DAG jobs with permissions, runner labels, network profile, artifact lane, timeout, and approval policy.")
     (step s6 :name schedule-runner-lane
       :logic "Resolve runner-pool-scheduler from runtime target profile: privatecloud for CN builds, ECS deploy-agent for CN deploy, GCP/global runners for global targets, Windows only for Windows/GPU/model tasks.")
     (step s6b :name macmini-local-build-lane
       :logic "For managed Mac nodes such as rickyhq-macmini-m4, prefer XJP codebase sync plus on-target cargo build/test/install over direct binary scp. Direct binary copy is bootstrap/break-glass only; steady state must report source commit, local build command, release path, health smoke, and rollback artifact.")
     (step s6c :name enforce-workspace-isolation
       :logic "Every implementation shard must specify workspace isolation, secret-ref injection, cache policy, cleanup, and result artifact; target-side GitHub/GHCR is forbidden on restricted CN runtimes.")
     (step s7 :name produce-artifact-provenance
       :logic "Close ArtifactRecord and ReleaseProvenance before declaring success: source commit, builder, workflow run/job, digest, storage ref, target/report digest, smoke result, rollback artifact.")
     (step s8 :name implement-exact-shards
       :logic "Dispatch ClaudeCode implementation workers only after accepted exact shards exist; broad 'replace GitHub Actions' goals are investigation/design tasks, not code-worker tasks.")
     (step s9 :name verify-and-promote
       :logic "Run deploy-center SSOT checker, focused DB/API tests, MissionD workflow gates, and one read-only workflow-run smoke before promoting a provider bridge out of compatibility mode.")
     (step s10 :name write-migration-report
       :logic "Record which projects still depend on GitHub Actions, which can use XJP native runner, which require Gitee/OSS domestic lane, and what operator decision remains."))
  :risk-gates
    ((gate g1 :rule "GitHub/Gitee are external providers or mirrors, not the XJP workflow authority.")
     (gate g2 :rule "No target-side GitHub/GHCR/DockerHub access for ecs-cn-restricted or synology-cn-restricted profiles.")
     (gate g3 :rule "No inline credentials; workflow permissions and runner secrets must be secret_ref-only.")
     (gate g4 :rule "A workflow run cannot close without task-result-artifact / ArtifactRecord / release provenance where applicable.")
     (gate g5 :rule "Compatibility bridges must declare owner, allowed read/write boundary, and exit condition.")
     (gate g6 :rule "Forge may suggest reusable components/templates but does not become runtime scheduler authority.")
     (gate g7 :rule "Managed Mac MissionD updates must prefer source sync + local build once bootstrap SSH is available; binary transfer is allowed only as a documented recovery bridge.")
     (gate g8 :rule "Pull/push/mirror/import/export requests must be durable CodebaseSyncOperation rows before deploy-agent or native runner executes them.")
     (gate g9 :rule "Source build caches must live outside mutable sync roots or be explicitly protected from sync --delete cleanup.")
     (gate g10 :rule "Codebase sync execution must reject inline remote credentials and use runner credential mounts or secret_ref-backed environment instead."))
  :completion
    ((criterion c1 :rule "deploy-center .missiond/check.sh validates dc-codebase-runner-blueprint and native control-plane migration.")
     (criterion c2 :rule "A native workflow dry-run can compile codebase facts into WorkflowDefinition, runner requirements, and artifact lane diagnostics without touching production.")
     (criterion c3 :rule "At least one low-risk project has a read-only native workflow smoke with durable events and no GitHub Actions completion dependency.")
     (criterion c4 :rule "All remaining GitHub Actions dependencies are listed as compatibility bridges with migration order and exit criteria.")))
