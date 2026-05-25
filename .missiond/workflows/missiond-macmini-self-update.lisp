(workflow missiond-macmini-self-update
  :workflow_id missiond-macmini-self-update
  :status active
  :schema "missiond.workflow.v1"
  :source_plans [interaction-gateway work-order-lifecycle xjp-native-codebase-runner-convergence m6-deployment-rollout]
  :match_rules ((title-prefix "MissionD Mac mini self-update")
                (objective-contains "Mac mini deploy-agent pulls source, builds locally, blue-green deploys MissionD")
                (trigger-kind version_bump_push)
                (project_id missiond)
                (target_id rickyhq-macmini-m4))
  :inputs [interaction_id? grounding_context_id intent_artifact_id plan_artifact_id source_commit target_id executor_name source_sync_provider acceptance]
  :defaults ((target_id rickyhq-macmini-m4)
             (executor_name macmini)
             (source_sync_provider github)
             (target_root "/Users/rickyhq/Projects/missiond")
             (deploy_command "scripts/deploy-daemon.sh --debug")
	             (slot_ensure_url "http://127.0.0.1:9120/internal/jarvis/slot/ensure")
	             (monitor_url "http://127.0.0.1:9120/api/monitor/jarvis"))

  :steps
    ((step s1 :id receive-client-objective
    :entry "Interaction Gateway receives a human or service request to update MissionD on Mac mini"
    :core ((step s1 :logic "require authenticated PermissionContext and persisted grounding_context_id")
           (step s2 :logic "draft intent.lisp explaining that Mac mini is the target runtime and source sync is GitHub first, XJP codebase later")
           (step s3 :logic "wait for intent confirmation unless workflow_id=missiond-macmini-self-update and exact_shard_ready=true was pre-approved"))
    :egress "intent_artifact_id + confirm_required"
       :surfaces ["crates/missiond-daemon/src/handlers/interaction" ".missiond/workflows/intent-intake-grounding.lisp"])

     (step s2 :id draft-deployment-plan
    :entry "confirmed intent artifact"
    :core ((step s1 :logic "generate plan.lisp with exact native workflow run stages")
           (step s2 :logic "bind project_id=missiond, target_id=rickyhq-macmini-m4, executor_name=macmini, source_commit, write_scope=[], acceptance")
           (step s3 :logic "wait for plan confirmation before creating deploy-center workflow run"))
    :egress "plan_artifact_id + confirm_required"
       :surfaces ["crates/missiond-daemon/src/handlers/interaction" ".missiond/workflows/work-order-lifecycle.lisp"])

     (step s3 :id create-native-workflow-run
    :entry "confirmed plan artifact"
    :core ((step s1 :logic "deploy-center script services/deploy-center/scripts/run-missiond-macmini-self-update.sh registers repository and workflow definition")
           (step s2 :logic "create WorkflowRun with source_commit, target_root, executor_name, correlation_id, interaction_id, intent_artifact_id, plan_artifact_id")
           (step s3 :logic "create jobs fetch-source, verify-source, build, blue-green-deploy, monitor-smoke, publish-provenance; each job is claimed by Mac mini deploy-agent"))
    :egress "WorkflowRun + WorkflowJob rows + BoardTask linked to deploy-center provenance"
       :surfaces ["services/deploy-center/scripts/run-missiond-macmini-self-update.sh" "services/deploy-center/src/api/codebase_runner.rs" "services/deploy-center/src/db/codebase_runner.rs"])

     (step s4 :id macmini-fetch-source
    :entry "Mac mini deploy-agent claims fetch-source job"
    :core ((step s1 :logic "synchronize /Users/rickyhq/Projects/missiond through GitHub or XJP codebase, never operator rsync or scp")
           (step s2 :logic "git fetch origin main, git checkout main, git pull --ff-only, verify git rev-parse HEAD equals source_commit")
           (step s3 :logic "if worktree is dirty outside external runtime dirs, fail with worktree_dirty diagnostic instead of overwriting"))
    :egress "source checkout at requested commit"
       :surfaces ["apps/xjp-deploy-agent/src/actors/puller.rs" "services/deploy-center/scripts/run-missiond-macmini-self-update.sh"])

     (step s5 :id verify-commit-version
    :entry "source checkout"
    :core ((step s1 :logic "run MissionD static gates that are practical on the target before deployment")
           (step s2 :logic "validate scripts/deploy-daemon.sh exists and release id can include source_commit")
           (step s3 :logic "write check output as workflow artifact, not Board note only"))
    :egress "verification artifact or typed failure"
       :surfaces ["scripts/check-v3-final-convergence.mjs" "scripts/check-v3-macmini-self-update-lane.mjs"])

     (step s6 :id target-build-test
    :entry "verified source"
    :core ((step s1 :logic "build on Mac mini with local Apple Silicon toolchain")
           (step s2 :logic "cargo build selected MissionD binaries with locked dependencies")
           (step s3 :logic "capture duration, stderr tail, cache diagnostics, and build artifacts in deploy-center provenance"))
    :egress "built MissionD binaries or build_failed diagnostic"
       :surfaces ["services/deploy-center/scripts/run-missiond-macmini-self-update.sh" "apps/xjp-deploy-agent/src/actors/puller.rs"])

     (step s7 :id blue-green-deploy
    :entry "build succeeded"
    :core ((step s1 :logic "run scripts/deploy-daemon.sh --debug on Mac mini target")
           (step s2 :logic "deploy script writes release id, active/previous symlink, rollback artifact, launchd kickstart result")
           (step s3 :logic "deploy script must verify generated V3 contracts instead of rewriting tracked generated files on the target")
           (step s4 :logic "failure must rollback or produce rollback_required diagnostic"))
    :egress "release id + rollback artifact + deploy-center workflow job report"
       :surfaces ["scripts/deploy-daemon.sh" "services/deploy-center/scripts/run-missiond-macmini-self-update.sh"])

     (step s8 :id monitor-smoke
    :entry "deployment job succeeded"
	    :core ((step s1 :logic "smoke http://127.0.0.1:9120/health from Mac mini")
	           (step s2 :logic "POST http://127.0.0.1:9120/internal/jarvis/slot/ensure to restore the default Exited/Error Jarvis slot exactly once after blue-green restart")
	           (step s3 :logic "smoke http://127.0.0.1:9120/api/monitor/jarvis from Mac mini and require overall=ready")
	           (step s4 :logic "verify git status --short --untracked-files=no remains empty after deploy and smoke; tracked generated diffs are a deployment failure")
	           (step s5 :logic "public Jarvis smoke may be run by Interaction Gateway after deploy-center reports local ready")
	           (step s6 :logic "monitor must explain daemon, MCP, slot readiness, and route diagnostics; empty 502/404 is a failure"))
    :egress "smoke_succeeded or typed diagnostic"
       :surfaces ["scripts/smoke-jarvis-chain.mjs" "scripts/smoke-jarvis-interaction.mjs"])

     (step s9 :id publish-provenance
    :entry "smoke result"
    :core ((step s1 :logic "deploy-center records source_commit, workflow_run_id, workflow_job ids, release id, duration, smoke result, rollback artifact")
           (step s2 :logic "EventBridge emits missiond_self_update_started/succeeded/failed and workflow_job_* events")
           (step s3 :logic "Interaction Gateway streams result_artifact and final back to iOS/Web/WeChat channel"))
    :egress "task-result-artifact + deploy-center provenance + final SSE event"
       :surfaces ["services/deploy-center/src/workers/deploy_event_relay.rs" "crates/missiond-daemon/src/handlers/interaction"]))

  :risk-gates ((gate g1 :rule "no-rsync-scp: Mac mini source sync must use GitHub or XJP codebase/deploy-center CodebaseSyncOperation; operator rsync/scp is forbidden outside break-glass")
               (gate g2 :rule "client-channel-required: Human broad requests must enter through Interaction Gateway and cannot directly create a deploy worker")
               (gate g3 :rule "master-not-implementer: resident master only drafts intent/plan and dispatches; deploy-agent executes the build/deploy job")
               (gate g4 :rule "secret-ref-only: tokens for deploy-center/Auth/secret-store are secret refs or target env only; values are never written to Lisp, Board, logs, or artifacts")
               (gate g5 :rule "task-result-artifact-required: completed workflow must produce task-result-artifact; PTY idle and Board note are projections")
               (gate g6 :rule "rollback-artifact-required: blue-green deploy must publish previous release or rollback marker before final success"))

  :completion ((criterion c1 :rule "local-monitor-ready: Mac mini /api/monitor/jarvis returns overall=ready")
               (criterion c2 :rule "public-jarvis-ready: public /jarvis/api/monitor/jarvis is either ready or returns typed tunnel diagnostic")
               (criterion c3 :rule "provenance-complete: deploy-center provenance links source_commit, workflow_run_id, release id, smoke result, rollback artifact")
               (criterion c4 :rule "interaction-final: requesting channel receives result_artifact and final event")))
