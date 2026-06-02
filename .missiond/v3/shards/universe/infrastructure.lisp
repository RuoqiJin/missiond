(eventbridge-policy
    :schema "missiond.eventbridge-policy.v1"
    :envelope missiond.event-envelope.v1
    :fields [event_id source project_id service_id event_kind subject correlation_id trace_id occurred_at observed_at authority schema_version payload privacy_class]
    :taxonomy [deploy_created build_started build_succeeded build_failed deploy_started deploy_succeeded deploy_failed smoke_succeeded smoke_failed rollback_started rollback_succeeded rollback_failed agent_heartbeat agent_offline agent_update_started agent_update_succeeded agent_update_failed provenance_changed usage_burst provider_error_burst provider_auth_failure_burst quota_exhaustion]
    :rule "MissionD remains the local orchestrator and EventBridge. Cloud services send durable provider events through typed webhooks; MissionD stores them as SystemEvent::ExternalServiceEvent with idempotent event_id dedupe. PTY remains diagnostic only."
    :invariants
      ["External deploy events MUST enter through /webhooks/deploy-center-event or /webhooks/service-event with X-MissionD-Webhook-Token when MISSIOND_EXTERNAL_WEBHOOK_TOKEN is configured."
       "Deploy-center event envelopes MUST carry stable event_id values derived from deploy-center durable rows; MissionD MUST reject deploy-center events without event_id."
       "mission_timeline(action=wait, domain=system) MUST support serviceId, eventKind, projectId, and correlationId predicates for deployment events."
       "ExternalServiceEvent append MUST use deterministic dedupe by service_id + event_id."])

(deployment-change-classification-policy
    :schema "missiond.deployment-change-classification-policy.v1"
    :entry [git-diff deploy-center.provenance ci-dispatcher xjp-workspace-ssot deploy-workflow-validation]
    :core ((step s1 :logic "classify a change set before any deployment fanout: service-runtime-change, workflow-only-change, ssot-checker-only, deploy-config-change, secret-dns-change, or unknown")
           (step s2 :logic "service-runtime-change may trigger the affected service deployment through deploy-center after normal provenance/smoke gates")
           (step s3 :logic "workflow-only changes, reusable deploy workflow changes, checker-only changes, and SSOT-only changes run validation-only workflows and must not fan out production service deployments")
           (step s4 :logic "deploy-config-change requires deploy-center provenance plus explicit rollout intent; secret-dns-change requires Decision Inbox or deploy-center policy before mutation")
           (step s5 :logic "unknown classification creates a diagnostic BoardTask/context-pack instead of guessing or dispatching deploy-ops workers"))
    :egress [deploy-intent validation-only-run deploy-rollout-suppression deploy-diagnostic-BoardTask]
    :surfaces [".missiond/workflows/deployment-event-response.lisp" "xjp-backend:.missiond/backend/xiaojinpro-backend-blueprint.lisp" "xjp-backend:.github/workflows/deploy-workflow-validation.yml"]
    :rule "Deployment work begins with change classification. A CI/workflow/tooling patch is not service runtime evidence and must be validated without causing broad production fanout. MissionD may observe and create diagnostics, but deploy-center remains the deployment fact authority.")

(m6-deployment-confirmation
    :schema "missiond.m6-deployment-confirmation.v1"
    :entry [project-maturity-registry service-runtime-universe deploy-center.status deploy-center.provenance]
    :core ((step s1 :logic "select projects whose project-maturity-registry current level is M6")
           (step s2 :logic "map each project to deploy-center service slug(s), for example auth→xjp-auth-center, deploy-center→xjp-deploy-center, router→xjp-router, pcea→pcea/pcea-api/pcea-video-vault")
           (step s3 :logic "query deploy-center /api/deploy/status and provenance surfaces; classify deployed-current, deployed-stale, not-confirmed, or deployed-unknown")
           (step s4 :logic "compare deployed commit to local service paths where the project lives in a git checkout; do not mark a service current when service-relevant files changed after the deployed commit")
           (step s5 :logic "distinguish CI/build/push success, deploy-center notify HTTP 200, deploy-center provenance, and service smoke; only provenance plus smoke can close deployment confirmation")
           (step s6 :logic "classify digest_resolution_failed, reported_digest_missing, runner_queued, build_cache_unavailable, and provenance_partial as typed diagnostics rather than burying them in free text")
           (step s7 :logic "order rollout through deploy-center before dependent services and emit a machine-readable deployment gap report"))
    :egress [m6-deployment-status-json deploy-ops-BoardTask m6-rollout-report]
    :surfaces ["scripts/check-m6-deployment-status.mjs" ".missiond/workflows/m6-deployment-rollout.lisp" ".missiond/workflows/pcea-deployment-rollout.lisp" "scripts/check-v3-project-registry-isomorphism.mjs"]
    :diagnostics [runner_queued build_cache_unavailable digest_resolution_failed reported_digest_missing provenance_partial]
    :rule "M6 maturity is not deployment evidence. Production deployment confirmation must come from deploy-center status/provenance and service smoke, with curl/git/GitHub probes only as diagnostics. Build-cache accelerators such as sccache/kellnr are performance aids and must not become implicit release blockers.")

(deployment-evidence-preflight
    :schema "missiond.deployment-evidence-preflight.v1"
    :entry [m6-deployment-rollout pcea-deployment-rollout mission_infra_query skill-runtime deploy-center.provenance]
    :core ((step s1 :logic "resolve project_id to MissionD Universe identity, project deployment SSOT, and deploy-center slug(s)")
           (step s2 :logic "collect skill-derived deployment evidence with include_kb=false, preserving source_skill/source_path/source_line and redacting credential-like values")
           (step s3 :logic "query deploy-center runtime/provenance and compare with skill evidence for host, agent, script, artifact, health, and rollback facts")
           (step s4 :logic "verify deploy-center pull-mode executor claim dependencies: deploy_executors.api_key_ref must resolve DEPLOY_AGENT_API_KEY from Secret Store on gcp-runtime before agent-offline or script-failure conclusions are trusted")
           (step s5 :logic "if skill evidence, deploy-center facts, Secret Store dependency health, and project SSOT disagree, create a drift diagnostic/Decision item and do not let deploy workers guess host, login path, script path, or agent project")
           (step s6 :logic "materialize a deploy context-pack for deploy-ops workers containing only reconciled facts, remaining unknowns, smoke commands, dependency-health evidence, and approval boundaries"))
    :egress [deploy-context-pack runtime-fact-drift deploy-ops-BoardTask]
    :surfaces [".missiond/workflows/m6-deployment-rollout.lisp" ".missiond/workflows/pcea-deployment-rollout.lisp" "crates/missiond-daemon/src/bus/v2_subscribers.rs" "scripts/check-v3-workflow-isomorphism.mjs"]
    :rule "Every deployment task must perform deployment-evidence-preflight before action. Skills are evidence and operational guidance; deploy-center provenance is deployment authority; MissionD orchestrates and records the decision path."
    :dependency-rule "Secret Store is the credential authority for deploy-center executor claim auth. Since 2026-05-11 its production runtime is ss.xiaojinpro.top on the GCP xjp-backend VM, not ClawCloud. If Secret Store is unreachable, classify deploy-blocked-by-secret-store against gcp-runtime/Caddy/docker/xjp-postgres health and surface namespace/key refs only; never expose credential values.")

(infrastructure-universe
    :schema "missiond.infrastructure-universe.v1"
    :rule "Servers, runtime targets, deployment locations, agent/executor facts, and skill-derived ops knowledge are first-class governance objects. MissionD owns the Universe summary and dispatch policy; deploy-center owns verified runtime/deployment facts; secret-store owns credential values; skills are evidence only."
    (runtime-target-contract
      :fields [target_id aliases kind environment owner_authority capabilities deploy_center_executor agent_url service_ids network_profile lan_group artifact_lanes evidence_refs freshness]
      :invariants ["Runtime targets promoted from skills MUST be marked unverified until deploy-center or an approved probe confirms them."
                   "MissionD workers encountering an unknown server MUST query mission_infra_query(action=skill_evidence|reconcile) before guessing login paths or deployment authority."
                   "Runtime facts from deploy-center provenance override local skill notes; MissionD never silently overwrites conflicts."])
    (credential-ref-contract
      :fields [secret_ref namespace key_name purpose required_capability]
      :invariants ["Lisp, Board notes, context packs, and skills MUST NOT become active stores for login passwords, API keys, Cloudflare tokens, or SSH secrets."
                   "mission_infra_query(action=credential_refs) returns secret refs and availability only; it never returns credential values."
                   "Credential-like skill lines are migration evidence and must be redacted before entering worker context."
                   "Provider account credentials such as Aliyun AccessKey are stored once as account-level secrets; capability targets such as DNS, OSS, ECS, or billing reference the account credential instead of duplicating narrower key names."])
    (skill-evidence-contract
      :fields [source_skill source_path source_line confidence last_verified_at promote_to credential_inline_risk excerpt]
      :rule "Skills are operational guidance and discovery evidence. A skill fact becomes active runtime truth only after reconcile promotes it into deploy-center runtime inventory or MissionD Universe with a source reference. mission_infra_query(action=skill_evidence|credential_refs) MUST apply explicit target_id/skill filters first; without explicit filters, context-gather deploy_ops calls MUST pass project_id/query and the infra lane MUST reject globally scanned skill evidence that does not match the project or query-specific terms.")
    (break-glass-runbook-contract
      :fields [runbook_id target_id service_id source_skill evidence_refs allowed_actions forbidden_actions credential_refs approval_required freshness]
      :rule "Manual ECS/SSH/operator fallback is a break-glass runbook, not the primary deploy path. It is attached to deploy-ops tasks only when deploy-center reports agent_offline/agent_update_failed or provenance cannot be obtained, and it must reference secret-store credential refs instead of inline secrets.")
    (read-only-remote-diagnostic-contract
      :fields [profile_id target_id service_id authority read_only allowed_operations forbidden_operations credential_refs event_sink artifact_sink]
      :profiles [deploy_provenance_snapshot container_inventory dependency_manifest_scan supply_chain_ioc_scan]
      :invariants ["Remote diagnostic work MUST resolve a deploy-center read-only diagnostic profile before touching agent endpoints. MissionD may list profile requirements with mission_infra_query(action=diagnostic_profiles), but it MUST NOT guess deploy-agent API keys or run raw agent exec for diagnostics."
                   "Allowed profile operations are restricted to deploy provenance snapshots, container inventory without env, dependency manifest file scans, and supply-chain IoC grep over already-present files. npm/pnpm/yarn/pip install, Python/Node import, lifecycle scripts, container env reads, mutating docker/system commands, and secret dumps are forbidden."
                   "Diagnostic output is stored as task-result-artifact or ExternalServiceEvent evidence; PTY/log output is a projection only and credential values are never returned."])
    (target-network-profile-contract
      :fields [profile_id allowed_outbound forbidden_outbound allowed_transfer_stores build_runtime_candidates target_side_build_allowed diagnostics]
      :invariants ["CN restricted targets such as Aliyun ECS and Synology/domestic-only VMs MUST NOT depend on target-side GitHub, GHCR, Docker Hub, or source builds."
                   "Privatecloud Ubuntu 10900KF, Windows 12900KF, and Synology VM share xjp-zibo-lan and may be used as build/cache/jump evidence when their agent/credential refs are healthy."
                   "Managed Mac nodes with enough local CPU such as rickyhq-macmini-m4 SHOULD receive source through GitHub or the XJP native codebase lane and build on target; direct rsync/scp/binary transfer from an operator laptop is a break-glass bootstrap path only."
                   "If a deploy worker sees a restricted target configured for GHCR/GitHub direct pull, it must create deployment-lane-mismatch instead of retrying network calls."])
    (artifact-delivery-lane-contract
      :fields [lane_id source_commit builder_id transfer_store target_runtime artifact_sha256 target_digest reported_digest rollback_artifact smoke_evidence]
      :lanes [cloud-registry-lane cn-oss-bundle-lane gitee-source-mirror-lane macmini-codebase-local-build-lane manual-break-glass-lane]
      :invariants ["cn-oss-bundle-lane means approved builder -> Aliyun OSS -> ECS internal download -> deploy-agent run -> reported digest; ECS must not build or pull GHCR as the normal path."
                   "macmini-codebase-local-build-lane means MissionD/deploy-center sync source and workflow definition through GitHub or XJP codebase to the managed Mac node, the node builds locally, signs/installs into its own ~/.xjp-mission release path, and reports build/test/health provenance. This lane is preferred after bootstrap because it avoids brittle operator-laptop file mirroring and proves the managed node can rebuild itself."
                   "gitee-source-mirror-lane is source/control evidence only unless paired with a builder and artifact lane."
                   "manual-break-glass-lane requires approval and post-action provenance."])
    (managed-source-sync-policy
      :entry [deploy-request remote-node-update missiond-work-order codebase-runner github-remote deploy-center.workflow-run]
      :core ((step s1 :logic "classify remote source update targets; managed Mac mini nodes and other capable build nodes must prefer GitHub or XJP codebase/deploy-center source synchronization")
             (step s2 :logic "after source synchronization, build, test, package, install, and smoke on the target node or on the deploy-center approved private-cloud builder, not on an operator laptop")
             (step s3 :logic "if deploy-center owns the rollout, GA/GitHub Actions may only act as control-plane trigger and must route actual build/deploy through the private-cloud/codebase/agent channel")
             (step s4 :logic "if an operator attempts rsync/scp source mirroring, classify it as break-glass diagnostic, require approval/provenance, and create a process-drift follow-up so the steady-state lane is repaired"))
      :egress [source-sync-provenance target-build-provenance deploy-center-workflow-run process-drift-diagnostic]
      :surfaces [".missiond/workflows/m6-deployment-rollout.lisp" "mission_infra_query(action=runtime_targets|skill_evidence)" "deploy-center.codebase_sync_operation" "deploy-center.workflow-run"])
    (agent-offline-response-policy
      :entry [deploy-center.agent_heartbeat deploy-center.agent_update_failed deployment-event-response mission_infra_query.skill_evidence mission_infra_query.diagnostic_profiles]
      :core ((step s1 :logic "when deploy-center emits agent_offline or repeated heartbeat/update failure, MissionD creates or updates one deploy-ops incident keyed by target_id/service_id/root_cause_key")
             (step s2 :logic "MissionD queries runtime target inventory and skill evidence for break-glass runbook refs such as PCEA ECS jump-host/OSS/deploy.sh facts, redacting any credential-like line")
             (step s3 :logic "MissionD first asks deploy-center for read-only diagnostic profiles such as deploy_provenance_snapshot, container_inventory, dependency_manifest_scan, and supply_chain_ioc_scan; unavailable credentials become Decision Inbox or secret-store binding gaps, not guessed raw agent calls")
             (step s4 :logic "resident master presents options: wait for agent recovery, trigger deploy-center self-update, run an approved read-only diagnostic profile, or use approved manual runbook; manual actions require explicit approval and deploy-ops worker context")
             (step s5 :logic "if a diagnostic profile or manual runbook is used, write evidence back to deploy-center/MissionD as provenance gap remediation instead of leaving an untracked shell operation"))
      :egress [deploy-ops-BoardTask break-glass-context-pack Decision-Inbox deploy-center-provenance-gap]
      :surfaces [".missiond/workflows/deployment-event-response.lisp" ".missiond/workflows/m6-deployment-rollout.lisp" "mission_infra_query(action=skill_evidence|credential_refs|diagnostic_profiles)"])
    (runtime-authority-map
      :authorities ((missiond :owns [project-identity universe-summary dispatch-policy eventbridge])
                    (deploy-center :owns [runtime-target-inventory executor-inventory service-deploy-location agent-heartbeat-provenance release-provenance])
                    (secret-store :owns [credential-values credential-rotation credential-availability])
                    (skills :owns [operational-guidance evidence-source workflow-procedure])
                    (forge :owns [component-catalog pattern-catalog code-reality-mirror])))
    (cloud-ops-delegation-policy
      :entry [operator-request mission_infra_query.credential_refs mission_infra_query.skill_evidence deployment-event-response m6-deployment-rollout]
      :core ((step s1 :logic "classify credential rotation, DNS changes, cloud account inventory, OSS/ECS setup, and deploy-agent recovery as cloud/deploy ops rather than generic coding work")
             (step s2 :logic "resident master builds a redacted context pack with target_id, credential_ref availability, skill evidence refs, intended mutation, rollback/verification command, and approval boundary")
             (step s3 :logic "delegate operational execution to the explicit claude-code-deploy-ops lane; Codex/resident master supervises, validates evidence, and updates SSOT/evidence, but does not perform routine shell/cloud console operations itself")
             (step s4 :logic "after one-off operation succeeds, promote reusable procedure into deploy-center/MissionD workflow evidence and keep only secret refs, not secret values"))
      :egress [deploy-ops-BoardTask cloud-ops-context-pack task-result-artifact ssot-evidence-update]
      :surfaces [".missiond/workflows/m6-deployment-rollout.lisp" ".missiond/workflows/deployment-event-response.lisp" "mission_infra_query(action=credential_refs|skill_evidence)"])
    (runtime-target :target_id gcp-runtime
      :aliases [gcp-production]
      :kind cloud-runtime
      :environment production
      :owner_authority deploy-center
      :capabilities [auth router deploy-center missiond-jarvis-edge secret-store credential-vault caddy-reverse-proxy production-runtime google-cloud-storage global-object-store]
      :service_ids [auth router deploy-center missiond-jarvis-edge secret-store global-object-store]
      :public_domain "ss.xiaojinpro.top"
      :public_ip "34.104.147.118"
      :credential_refs [secret-store://cloud/gcp/deploy-center-runtime secret-store://deploy-agent/gcp/DEPLOY_AGENT_API_KEY secret-store://secret-store/cloudflare/CLOUDFLARE_DNS_TOKEN]
      :diagnostic_profiles [deploy_provenance_snapshot container_inventory dependency_manifest_scan supply_chain_ioc_scan]
      :freshness verified-2026-05-11
      :evidence_refs [service-runtime-universe deploy-center-provenance secret-store-gcp-migration-20260511 gcp-global-object-store-20260513 jarvis-xiaojinpro-top-cloudflare-dns-20260528])
    (runtime-target :target_id aliyun-account
      :aliases [aliyun-global aliyun-cloud-account]
      :kind cloud-account
      :environment cn-production
      :owner_authority deploy-center
      :capabilities [alidns oss ecs ram cloud-account-inventory domain-record-inventory domain-record-upsert object-storage-bucket-management ecs-runtime-management]
      :service_ids [long-image-service pcea secret-store-cn deploy-center-cn]
      :freshness credential-rotated-and-dns-read-verified-2026-05-13
      :credential_refs [secret-store://aliyun-global/ALIYUN_ACCESS_KEY_ID secret-store://aliyun-global/ALIYUN_ACCESS_KEY_SECRET]
      :evidence_refs [aliyun-global-access-key-rotation-20260513 skill:aliyun])
    (runtime-target :target_id aliyun-dns
      :aliases [aliyun-alidns changtu-pro-dns]
      :kind dns-provider
      :environment cn-production
      :owner_authority deploy-center
      :capabilities [domain-record-inventory domain-record-upsert changtu-pro-dns]
      :service_ids [long-image-service pcea]
      :freshness dns-read-verified-2026-05-13
      :credential_refs [secret-store://aliyun-global/ALIYUN_ACCESS_KEY_ID secret-store://aliyun-global/ALIYUN_ACCESS_KEY_SECRET]
      :evidence_refs [aliyun-global-access-key-rotation-20260513 changtu-pro-deployment-and-payment-boundary-20260513 skill:aliyun])
    (runtime-target :target_id ecs-pcea
      :aliases [pcea-ecs]
      :kind cloud-vm
      :environment production
      :owner_authority deploy-center
      :capabilities [pcea deploy-agent runtime secret-store-cn long-image-service]
      :service_ids [pcea secret-store-cn long-image-service]
      :network_profile ecs-cn-restricted
      :artifact_lanes [cn-oss-bundle-lane gitee-source-mirror-lane manual-break-glass-lane]
      :freshness verified-runtime-smoke-2026-05-15
      :runtime_facts (instance_id "i-uf6641fl52xo7ukf7kgl"
                      instance_name "iZuf6641fl52xo7ukf7kglZ"
                      public_ip "106.15.2.17"
                      zone "cn-shanghai-e"
                      agent_version "10.7.2"
                      current_containers [pcea-app pcea-api pcea-postgres secret-store-cn-app long-image-service]
                      runtime_smoke [secret-store-cn-livez secret-store-cn-readyz long-image-public-icp-blocked long-image-host-header-stale]
                      public_domain_blocks [changtu-pro-icp])
      :break_glass_runbook_refs [skill:pcea#ssh skill:pcea#deploy skill:aliyun#ECS skill:deploy-ops#deploy-agent]
      :credential_refs [secret-store://deploy-agent/DEPLOY_AGENT_ECS_API_KEY secret-store://infra/aliyun-ecs/ssh]
      :diagnostic_profiles [deploy_provenance_snapshot container_inventory dependency_manifest_scan supply_chain_ioc_scan]
      :evidence_refs [skill:pcea skill:aliyun skill:deploy-ops secret-store-cn-ecs-deploy-20260513 secret-store-cn-runtime-verified-20260515 changtu-pro-cn-deployment-20260513])
    (runtime-target :target_id privatecloud-10900kf
      :aliases [privatecloud privatecloud-lan-192-168-1-20 ubuntu-10900kf]
      :kind local-lan-builder
      :environment local-lan
      :owner_authority deploy-center
      :deploy_center_executor privatecloud
      :agent_url privatecloud
      :capabilities [cn-build cache harbor github-runner deploy-agent domestic-jump]
      :service_ids []
      :network_profile privatecloud-build-lan
      :lan_group xjp-zibo-lan
      :artifact_lanes [cn-oss-bundle-lane gitee-source-mirror-lane]
      :freshness declared-2026-05-13-agent-offline
      :credential_refs [secret-store://deploy-agent/DEPLOY_AGENT_API_KEY]
      :evidence_refs [skill:private-cloud user-topology-20260513])
    (runtime-target :target_id privatecloud-hostvds
      :aliases [hostvds privatecloud]
      :kind vps-runtime
      :environment privatecloud
      :owner_authority deploy-center
      :capabilities [deploy tunnel runtime]
      :service_ids []
      :freshness unverified
      :evidence_refs [skill:missiond-memory skill:xjp-deploy-center])
    (runtime-target :target_id windows-12900kf
      :aliases [12900kf windows-runner]
      :kind windows-workstation
      :environment local-lan
      :owner_authority deploy-center
      :deploy_center_executor windows
      :agent_url windows
      :capabilities [gpu github-runner embedding rerank deploy-agent]
      :service_ids [router]
      :network_profile privatecloud-build-lan
      :lan_group xjp-zibo-lan
      :freshness skill-derived-unverified
      :credential_refs [secret-store://deploy-agent/windows-12900kf/agent-token]
      :evidence_refs [skill:windows-runner skill:missiond-model-routing])
    (runtime-target :target_id rickyhq-macmini-m4
      :aliases [rickyhqmac-mini macmini-managed-node macmini-missiond-worker]
      :kind managed-mac-node
      :environment local-lan
      :owner_authority missiond
      :deploy_center_executor macmini
      :agent_url rickyhqmac-mini
      :capabilities [missiond-daemon mission-mcp claude-code codex-cli gemini-cli local-rust-build codebase-runner local-blue-green homebrew-managed-toolchain postgres-client]
      :service_ids [missiond]
      :network_profile mac-managed-node
      :artifact_lanes [macmini-codebase-local-build-lane manual-break-glass-lane]
      :freshness health-verified-2026-05-18
      :runtime_facts (hostname "RickyHQdeMac-mini.local"
                      user "rickyhq"
                      project_root "/Users/rickyhq/Projects/missiond"
                      runtime_root "/Users/rickyhq/.xjp-mission"
                      health "http://127.0.0.1:9120/health"
                      launchd_label "com.missiond.daemon"
                      local_build_capability true
                      bootstrap_package_manager "homebrew"
                      required_diagnostic_clis [psql]
                      postgres_client_package "libpq"
                      postgres_client_paths ["/opt/homebrew/opt/libpq/bin" "/usr/local/opt/libpq/bin" "/opt/homebrew/bin" "/usr/local/bin"]
                      bootstrap_note "direct binary transfer is allowed only for initial repair; steady state should use codebase sync plus local build")
      :credential_refs [secret-store://managed-node/rickyhq-macmini/ssh secret-store://managed-node/rickyhq-macmini/claude]
      :diagnostic_profiles [deploy_provenance_snapshot container_inventory dependency_manifest_scan supply_chain_ioc_scan]
      :evidence_refs [work-order:20260516-macmini-managed-node skill:rickyhqmac-mini])
    (runtime-target :target_id synology-astrill-gw
      :aliases [synology-vm astrill-gw domestic-jump]
      :kind local-lan-gateway
      :environment local-lan
      :owner_authority deploy-center
      :capabilities [domestic-jump network-gateway]
      :service_ids []
      :network_profile synology-cn-restricted
      :lan_group xjp-zibo-lan
      :artifact_lanes [manual-break-glass-lane]
      :freshness declared-2026-05-13-credential-ref-required
      :credential_refs [secret-store://infra/synology-astrill-gw/ssh]
      :evidence_refs [skill:astrill-gateway user-topology-20260513])
    (runtime-target :target_id bwg-vps
      :aliases [bwg model-tunnel]
      :kind vps-tunnel
      :environment relay
      :owner_authority deploy-center
      :capabilities [tunnel router-relay model-relay]
      :service_ids [router]
      :freshness skill-derived-unverified
      :credential_refs [secret-store://infra/bwg-vps/tunnel-ssh]
      :evidence_refs [skill:missiond-model-routing])
    (runtime-target :target_id privatecloud-lan-192-168-1-20
      :aliases [lan-infra harbor-cache]
      :kind local-lan-node
      :environment local-lan
      :owner_authority deploy-center
      :capabilities [cache harbor dns registry]
      :service_ids []
      :network_profile privatecloud-build-lan
      :lan_group xjp-zibo-lan
      :freshness skill-derived-unverified
      :evidence_refs [skill:xjp-deploy-center])
    :surfaces ["crates/missiond-daemon/src/handlers/sysinfra/infra.rs"
               "crates/missiond-daemon/src/handlers/knowledge/project/reconcile.rs"
               "crates/missiond-mcp/src/tools/sysinfra/infra.rs"
               "packages/board/src/app/api/infra/route.ts"
               "packages/board/src/components/SystemDashboard.tsx"
               "scripts/check-v3-infrastructure-universe-isomorphism.mjs"])

(deploy-agent-self-update-governance
    :schema "missiond.deploy-agent-self-update-governance.v1"
    :owner deploy-center
    :authority-table deploy_agent_update_provenance
    :facts [agent_id current_version desired_version s3_latest update_status canary_status rollback_marker last_error]
    :events [agent_update_started agent_update_succeeded agent_update_failed rollback_started rollback_succeeded rollback_failed agent_heartbeat agent_offline]
    :rule "deploy-agent self-update and reachability status are deploy-center runtime facts stored in deploy_agent_update_provenance and heartbeat/provenance tables; deploy-center relays update/offline events into MissionD EventBridge so deploy-ops BoardTasks can be triggered from durable events. A failed best-effort notify must not be hidden inside a globally successful release summary: per-agent failure remains actionable until the target reports the desired version or an approved break-glass runbook closes the incident.")
