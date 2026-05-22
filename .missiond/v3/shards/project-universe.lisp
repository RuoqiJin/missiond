  (project-registry-policy
    :desc "Lisp-owned project registry defaults for intent discovery and universe import."
    :intent-path-candidates [".missiond/intent.lisp" ".jarvis/intent.lisp" "intent.lisp"]
    :default-universe-manifest "/Users/jinchen/Projects/universe.intent.lisp"
    :env-overrides [UNIVERSE_MANIFEST]
    :invariants
      ["mission_project init/import_universe/survey MUST project intent-path candidates from project-registry-policy."
       "mission_project import_universe MUST project its default manifest from project-registry-policy; UNIVERSE_MANIFEST is only an explicit override."
       "A real MissionD project with .missiond but no project-registry-policy MUST return V3_BLUEPRINT_CONFIG_ERROR rather than silently using embedded defaults."])

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

  (eventhub-service-contract
    :schema "missiond.eventhub-service-contract.v1"
    :service-id xjp-eventhub
    :purpose "Extract cross-service durable event storage, waits, subscriptions, cursors, and replay into an XJP backend service while preserving MissionD's local low-latency EventBus for agent/Board/slot/workflow control."
    :ownership
      ((owner missiond-local-eventbus
        :owns [agent-events board-events slot-events workflow-events pty-diagnostics local-wakeups]
        :rule "Local orchestration must continue when xjp-eventhub is unavailable; local events are spooled for later outbound relay when configured.")
       (owner xjp-eventhub
        :owns [durable-event-envelope stream-cursors subscriptions wait-predicates dead-letter-replay cross-service-events]
        :runtime-env [MISSIOND_EVENTHUB_URL MISSIOND_EVENTHUB_TOKEN]
        :rule "xjp-eventhub is the cloud/service event backbone for deploy-center, auth, router, timeline, and selected MissionD local events.")
       (owner deploy-center
        :owns [deployment-provenance deploy-agent-events rollout-events]
        :rule "deploy-center remains deployment fact authority; eventhub stores and distributes its emitted facts but does not infer release state."))
    :event-envelope
      (schema missiond.event-envelope.v1
        :fields [event_id source project_id service_id event_kind subject correlation_id trace_id occurred_at observed_at authority schema_version payload privacy_class]
        :idempotency [source event_id]
        :privacy-classes [public internal private secret-redacted])
    :functions
      ((function eventhub-service-boundary
         :entry [MissionD-local-EventBus deploy-center-events auth-events router-events timeline-events]
         :core ((step s1 :logic "classify event as local-control, cross-service, or diagnostic")
                (step s2 :logic "store local-control events in MissionD local bus first")
                (step s3 :logic "spool selected events to xjp-eventhub with source/event_id idempotency")
                (step s4 :logic "preserve MissionD offline operation when xjp-eventhub is unavailable"))
         :egress [local-event-bus outbound-spool xjp-eventhub-event])
       (function local-event-spool
         :entry [EventBus-publish outbound-relay-tick]
         :core ((step s1 :logic "persist selected local events with cursor and retry metadata")
                (step s2 :logic "redact payload fields by privacy_class before relay")
                (step s3 :logic "relay to xjp-eventhub when endpoint and token are configured")
                (step s4 :logic "mark delivered, retryable, or dead-letter without blocking local MissionD workflows"))
         :egress [spool-row relay-diagnostic])
       (function eventhub-wait-contract
         :entry [mission_timeline.wait eventhub.wait]
         :core ((step s1 :logic "resolve predicate over project_id/service_id/event_kind/correlation_id/trace_id")
                (step s2 :logic "prefer local EventBus for local-control predicates")
                (step s3 :logic "use xjp-eventhub for cross-service predicates when configured")
                (step s4 :logic "fall back to bounded local event_log polling with visible diagnostic only when eventhub is unavailable"))
         :egress [wait-result timeout-diagnostic]))
    :runtime-projection [xjp-eventhub service-runtime-universe eventbridge local-event-spool mission_timeline.wait mission_timeline.eventhub_status mission_timeline.eventhub_query mission_timeline.eventhub_append]
    :checker "node scripts/check-v3-service-extraction-isomorphism.mjs")

  (provider-runtime-bringup-contract
    :schema "missiond.provider-runtime-bringup.v1"
    :purpose "Make local xjp-memory and xjp-eventhub provider runtime reproducible instead of relying on one-off LaunchAgent edits."
    :script "scripts/manage-local-providers.sh"
    :services
      ((provider xjp-memory
         :label "com.xjp.memory.provider"
         :url "http://127.0.0.1:8091"
         :database "xjp_memory"
         :storage postgres-durable
         :missiond-env [MISSIOND_MEMORY_PROVIDER_URL MISSIOND_MEMORY_PROVIDER_MODE])
       (provider xjp-eventhub
         :label "com.xjp.eventhub.provider"
         :url "http://127.0.0.1:8092"
         :database "xjp_eventhub"
         :storage postgres-durable
         :missiond-env [MISSIOND_EVENTHUB_URL MISSIOND_EVENTHUB_MODE]))
    :functions
      ((function local-provider-launchd
         :entry [developer-local-install scripts.manage-local-providers.install launchd]
         :core ((step s1 :logic "build xjp-memory and xjp-eventhub from the canonical XJP monorepo")
                (step s2 :logic "ensure local Postgres databases xjp_memory and xjp_eventhub exist")
                (step s3 :logic "write LaunchAgent plists for com.xjp.memory.provider and com.xjp.eventhub.provider")
                (step s4 :logic "wire MissionD LaunchAgent provider env to 127.0.0.1 service URLs")
                (step s5 :logic "bootstrap providers and MissionD, then smoke provider status endpoints"))
         :egress [launchd-plist missiond-provider-env provider-smoke-report])
       (function local-provider-smoke
         :entry [scripts.manage-local-providers.smoke mission_memory.provider_status mission_timeline.eventhub_status]
         :core ((step s1 :logic "query /v1/memory/provider_status and require postgres-durable storage")
                (step s2 :logic "query /v1/eventhub/status and require postgres-durable storage")
                (step s3 :logic "surface provider diagnostics through MCP instead of assuming local compatibility mode"))
         :egress [provider-status diagnostic]))
    :invariants
      ["Local provider enablement MUST be reproducible through scripts/manage-local-providers.sh install, not manual plist editing."
       "The script MUST not store secrets; provider tokens remain in secret-store/env and may be injected separately."
       "MissionD must continue to support null/local compatibility providers when xjp-memory or xjp-eventhub are not configured."]
    :checker "node scripts/check-v3-service-extraction-isomorphism.mjs")

  (deployment-event-ingest
    :schema "missiond.deployment-event-ingest.v1"
    :entry [/webhooks/deploy-center-event mission_timeline.wait deployment-event-response.workflow]
    :core ((step s1 :logic "validate optional MissionD webhook token and parse missiond.event-envelope.v1")
           (step s2 :logic "preserve event identity and project/correlation fields under payload._envelope")
           (step s3 :logic "publish SystemEvent::ExternalServiceEvent through EventBus with service/event dedupe")
           (step s4 :logic "allow master, Autopilot, and deploy-ops workflows to wait by service_id, event_kind, project_id, and correlation_id")
           (step s5 :logic "create BoardTask suggestions for deploy/smoke/agent-offline/agent-update failures; attach break-glass runbook refs when the deploy agent is unreachable, but never auto rollback, SSH, DNS mutate, secret mutate, or production-deploy without deploy-center policy or user approval"))
    :egress [ExternalServiceEvent mission_timeline.wait deployment-ops-BoardTask]
    :surfaces [eventbridge project-registry])

  (router-usage-event-ingest
    :schema "missiond.router-usage-event-ingest.v1"
    :entry [/webhooks/service-event mission_timeline.wait router-usage-alert]
    :core ((step s1 :logic "accept router service-event envelopes for usage_burst, provider_error_burst, provider_auth_failure_burst, and quota_exhaustion without treating PTY as evidence")
           (step s2 :logic "preserve caller attribution fields project_id, service_id, provider, model, route, request_id, tenant_id hash, status_code, and error class")
           (step s3 :logic "dedupe burst alerts by service_id + provider + model + window_start + event_kind")
           (step s4 :logic "surface repeated provider/auth failures to Board as diagnostic incidents and do not retry hidden translation or background LLM work")
           (step s5 :logic "allow mission_timeline waits and master-control observers to react to router anomalies from durable events"))
    :egress [ExternalServiceEvent router-usage-diagnostic router-ops-BoardTask]
    :surfaces [eventbridge router-policy])

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

  (project-identity-contract
    :schema "missiond.project-identity-contract.v1"
    :fields [project_id canonical_root repo_remote ssot_paths deploy_center_slug forge_project_name service_ids aliases status]
    :rule "MissionD is project identity and SSOT registry authority; deploy-center is deployment fact authority; Forge is component/pattern/reality catalog authority."
    :reconcile-action mission_project.reconcile
    :invariants
      ["MissionD Universe owns canonical project ids, roots, SSOT paths, maturity, Board links, and workstation dispatch."
       "deploy-center owns deployment targets, runtime location, release provenance, deploy agents, and executor state."
       "Forge owns component/pattern catalog, code reality mirror, and Universe DAG recommendations; Forge-only references are not deployable unless MissionD registers them."
       "Historical aliases such as xjp-deploy-center MUST NOT become active project roots."])

  (registry-authority-map
    :schema "missiond.registry-authority-map.v1"
    :authorities ((missiond :owns [project-identity ssot-paths maturity board-workstation-scheduling])
                  (deploy-center :owns [deployment-targets runtime-location release-provenance agent-executor-state])
                  (forge :owns [component-catalog pattern-catalog code-reality-mirror universe-dag-recommendations]))
    :workflow project-registry-reconciliation
    :rule "Registry reconciliation reads MissionD, deploy-center, and Forge facts, reports missing_in_*, alias_conflict, root_mismatch, and deploy_fact_missing, and never silently overwrites identities.")

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
      :rule "Skills are operational guidance and discovery evidence. A skill fact becomes active runtime truth only after reconcile promotes it into deploy-center runtime inventory or MissionD Universe with a source reference.")
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
                   "Managed Mac nodes with enough local CPU such as rickyhq-macmini-m4 SHOULD receive source through the XJP native codebase lane and build on target; direct binary scp is a break-glass bootstrap path only."
                   "If a deploy worker sees a restricted target configured for GHCR/GitHub direct pull, it must create deployment-lane-mismatch instead of retrying network calls."])
    (artifact-delivery-lane-contract
      :fields [lane_id source_commit builder_id transfer_store target_runtime artifact_sha256 target_digest reported_digest rollback_artifact smoke_evidence]
      :lanes [cloud-registry-lane cn-oss-bundle-lane gitee-source-mirror-lane macmini-codebase-local-build-lane manual-break-glass-lane]
      :invariants ["cn-oss-bundle-lane means approved builder -> Aliyun OSS -> ECS internal download -> deploy-agent run -> reported digest; ECS must not build or pull GHCR as the normal path."
                   "macmini-codebase-local-build-lane means MissionD/deploy-center sync source and workflow definition to the managed Mac node, the node builds locally, signs/installs into its own ~/.xjp-mission release path, and reports build/test/health provenance. This lane is preferred after bootstrap because it avoids brittle large binary transfer and proves the managed node can rebuild itself."
                   "gitee-source-mirror-lane is source/control evidence only unless paired with a builder and artifact lane."
                   "manual-break-glass-lane requires approval and post-action provenance."])
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
      :capabilities [auth router deploy-center secret-store credential-vault caddy-reverse-proxy production-runtime google-cloud-storage global-object-store]
      :service_ids [auth router deploy-center secret-store global-object-store]
      :public_domain "ss.xiaojinpro.top"
      :public_ip "34.104.147.118"
      :credential_refs [secret-store://cloud/gcp/deploy-center-runtime secret-store://deploy-agent/gcp/DEPLOY_AGENT_API_KEY secret-store://secret-store/cloudflare/CLOUDFLARE_DNS_TOKEN]
      :diagnostic_profiles [deploy_provenance_snapshot container_inventory dependency_manifest_scan supply_chain_ioc_scan]
      :freshness verified-2026-05-11
      :evidence_refs [service-runtime-universe deploy-center-provenance secret-store-gcp-migration-20260511 gcp-global-object-store-20260513])
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
      :capabilities [missiond-daemon mission-mcp claude-code codex-cli gemini-cli local-rust-build codebase-runner local-blue-green]
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

  (data-residency-universe
    :schema "missiond.data-residency-universe.v1"
    :purpose "Govern legal/technical data partitions for data-bearing projects. This is an architecture SSOT, not legal advice: it makes region identity, issuer, secrets, storage, payment, model routing, and cross-region egress explicit so MissionD can refuse ambiguous M6 releases."
    :research ".missiond/research/data-residency-universe-report-20260512.md"
    :rule "cn and global are hard partitions at the XJP platform layer. Applications such as PCEA and CUTHUB bind to xjp-cn or xjp-global instead of inventing separate auth/secret/payment/router stacks. global-eu is an operating zone inside xjp-global until a project explicitly declares a separate hard partition. Region routing uses explicit project/workspace selection plus account/payment signals; IP is a hint only."
    (data-region-partition-contract
      :partition-key project-or-workspace-id
      :partitions
        ((partition cn
           :boundary hard
           :authority "China mainland legal entity / ICP / local runtime"
           :identity-namespace cn
           :runtime "mainland cloud runtime; exact deploy-center target required before launch"
           :must-not-share [issuer signing-key kek payment-ledger object-storage vector-db prompt-corpus eventhub user-table])
         (partition global
           :boundary hard
           :authority "global legal entity / non-mainland runtime"
           :identity-namespace global
           :operating-zones [global-us global-eu]
           :must-not-share-with [cn])
         (operating-zone global-eu
           :parent global
           :boundary soft-plus
           :authority "GDPR/EU data boundary inside global"
           :pins [storage kms logs support-access customer-data])
         (operating-zone global-us
           :parent global
           :boundary default
           :authority "global US default runtime"))
	      :invariants ["Partition id MUST appear in issuer, audience, storage bucket prefix, KMS/KEK id, event topic, payment account, and router policy."
	                   "A token, API key, storage credential, model prompt route, or payment webhook from one hard partition MUST NOT be accepted by another hard partition."
	                   "Cross-partition movement requires a fresh authentication/export flow and must be classified by cross-region-data-policy."])
    (xjp-platform-partition-contract
      :fields [platform-partition legal-region runtime-target runtime-provider deploy-center-agent auth-stack secret-store payment-ledger storage-ledger router-policy eventhub timeline deploy-center-lane application-bindings status]
      :rule "XJP infrastructure, not each application, owns the cn/global separation. New data-bearing applications bind to a platform partition and inherit its auth, secret-store, payment, storage, router, event, deployment, and observability boundaries."
      :partitions
        ((xjp-cn
           :legal-region cn
           :runtime-target ecs-pcea
           :runtime-provider aliyun-ecs
           :deploy-center-agent ecs
           :auth-stack auth-cn
           :secret-store secret-store-cn
           :payment-ledger xjp-cn-ledger
           :storage-ledger xjp-cn-storage
           :router-policy xjp-cn-router
           :eventhub xjp-cn-eventhub
           :timeline xjp-cn-timeline
           :deploy-center-lane xjp-cn-deploy
           :application-bindings [pcea-cn cuthub-cn]
           :status active-cn-platform)
         (xjp-global
           :legal-region global
           :runtime-target gcp-runtime
           :runtime-provider gcp-vm
           :deploy-center-agent gcp
           :auth-stack auth-global
           :secret-store secret-store-global
           :payment-ledger xjp-global-ledger
           :storage-ledger (xjp-global-storage :provider google-cloud-storage :bucket "gs://xjp-global-object-store-project-20250408" :location ASIA :ubla true)
           :router-policy xjp-global-router
           :eventhub xjp-global-eventhub
           :timeline xjp-global-timeline
           :deploy-center-lane xjp-global-deploy
           :application-bindings [pcea-global cuthub-global]
           :status active-global-platform)
         (xjp-global-eu
           :parent xjp-global
           :legal-region eu
           :runtime-target gcp-runtime
           :runtime-provider gcp-vm
           :deploy-center-agent gcp
           :auth-stack auth-global
           :storage-ledger xjp-global-eu-storage
           :router-policy xjp-global-eu-router
           :eventhub xjp-global-eu-eventhub
           :application-bindings [pcea-global-eu]
           :status operating-zone-pending-dedicated-eu-runtime))
      :invariants ["Applications bind to exactly one hard platform partition for active user data; dual-homed user records are forbidden."
                   "An app-level partition may narrow storage/model/payment policy, but it cannot weaken the platform partition boundary."
                   "Deploy Center must expose platform-partition release provenance before MissionD can mark an app-region target deployed."])
    (regional-auth-issuer-contract
      :fields [partition issuer jwks audience oauth-clients token-signing-key session-store account-link-policy]
      :pcea ((pcea-cn :issuer "https://auth.pcea.cn" :jwks "https://auth.pcea.cn/.well-known/jwks.json" :audience pcea-cn :account-link-policy separate-account)
             (pcea-global :issuer "https://auth.pcea.io" :jwks "https://auth.pcea.io/.well-known/jwks.json" :audience pcea-global :account-link-policy separate-account)
             (pcea-global-eu :issuer "https://auth.pcea.io" :audience pcea-global-eu :session-store eu-pinned))
      :cuthub ((cuthub-cn :issuer "https://auth.cuthub.cn" :domain "cuthub.cn" :account-link-policy separate-account)
               (cuthub-global :issuer "https://auth.cuthub.com" :domain "cuthub.com" :account-link-policy separate-account))
      :forbidden [cross-partition-token-trust parent-domain-cookie-sharing shared-jwks-between-cn-and-global])
    (regional-secret-store-contract
      :fields [partition secret-store-url secret_ref_namespace kek_id kms_provider rotation_policy break_glass_policy]
      :rule "Secret values live in secret-store only. Lisp records namespaced secret refs and region/KMS ownership; it never carries values."
      :pcea ((pcea-cn :secret-namespace "pcea/cn" :kek "pcea-cn-kek" :kms-provider mainland-kms)
             (pcea-global-us :secret-namespace "pcea/global/us" :kek "pcea-global-us-kek" :kms-provider aws-kms-us)
             (pcea-global-eu :secret-namespace "pcea/global/eu" :kek "pcea-global-eu-kek" :kms-provider aws-kms-eu)))
    (regional-runtime-target-contract
      :fields [partition runtime-target runtime-provider deploy-center-agent public-domain deploy-mode artifact-flow release-provenance smoke rollback]
      :rule "Data partitions must bind to explicit deploy-center runtime targets before production rollout. MissionD records the intended placement and refuses to infer region placement from IP, domain, git branch, or a stale deploy row."
      :pcea ((pcea-cn :runtime-target ecs-pcea :runtime-provider aliyun-ecs :deploy-center-agent ecs :public-domain "pcea.top" :deploy-mode current-production-cn-compatible :artifact-flow [github-actions privatecloud-runner oss-cn-shanghai deploy-center ecs-agent] :must-not-build-on-target true)
             (pcea-global :runtime-target gcp-runtime :runtime-provider gcp-vm :deploy-center-agent gcp :public-domain "pcea.io" :deploy-mode target-pending-provisioning :requires [deploy-center-project secret-store-namespace storage-ledger payment-ledger auth-issuer smoke-rollout])
             (pcea-global-eu :runtime-target gcp-runtime :runtime-provider gcp-vm :deploy-center-agent gcp :deploy-mode operating-zone-pending-dedicated-eu-runtime :requires [eu-storage-kms-support-access-pinning]))
      :invariants ["CN PCEA production traffic targets Aliyun ECS until an explicit deploy-center migration task proves otherwise."
                   "Global PCEA traffic targets the GCP VM lane, but it MUST use a separate deploy-center project/release provenance from the CN/ECS lane."
                   "A global rollout may reuse source code and container templates, but MUST NOT reuse CN secrets, ledgers, object stores, vector stores, or user tables."])
    (regional-storage-contract
      :fields [partition object-store vector-store database log-store backup-store encryption-key data-classification]
      :rule "User files, subtitles, RAG chunks, embeddings, prompts, logs, and backups are region-pinned. Code, templates, and compiled artifacts are not user data and may be mirrored globally."
      :pcea ((pcea-cn :object-store oss-cn :vector-store milvus-cn :database postgres-cn :log-store log-cn)
             (pcea-global-us :object-store gcs-global-asia :bucket "gs://xjp-global-object-store-project-20250408" :vector-store pgvector-us :database postgres-us :log-store log-us)
             (pcea-global-eu :object-store gcs-global-eu-pending :bucket "gs://xjp-global-object-store-project-20250408/eu-pending" :vector-store pgvector-eu :database postgres-eu :log-store log-eu)))
    (regional-payment-ledger-contract
      :fields [partition legal-entity payment-provider currency ledger-db tax-invoice-policy allowed-aggregate-egress]
      :rule "Payment ledgers are hard-partitioned because acquiring, settlement, tax, invoice, and AML obligations differ by region. Cross-region finance egress is monthly aggregate only, never transaction detail."
      :pcea ((pcea-cn :provider [wechat-pay alipay mainland-psp] :currency CNY :invoice fapiao :ledger pcea-cn-ledger)
             (pcea-global-us :provider stripe-us :currency USD :ledger pcea-global-us-ledger)
             (pcea-global-eu :provider stripe-eu :currency EUR :ledger pcea-global-eu-ledger)))
    (regional-router-model-policy
      :fields [partition allowed_models denied_models pii_prompt_policy embedding_region rerank_region rag_corpus_region model_audit_log]
      :rule "Prompt and embedding data inherit the region of the project/workspace. Mainland partitions use mainland-approved models. EU user data uses EU-pinned providers or zero-data-retention agreements. DeepSeek/Qwen/Doubao are denied for global PI unless a project-specific privacy review allows a non-PI path."
      :pcea ((pcea-cn :allowed_models [qwen doubao ernie kimi] :denied_models [openai-public anthropic-public deepseek-for-pi] :pii_prompt_policy cn-only)
             (pcea-global-us :allowed_models [openai-us anthropic-bedrock-us gemini-vertex-us] :denied_models [qwen-for-pi doubao-for-pi deepseek-for-pi] :pii_prompt_policy us-global)
             (pcea-global-eu :allowed_models [openai-eu anthropic-bedrock-eu gemini-vertex-eu] :denied_models [qwen-for-pi doubao-for-pi deepseek-for-pi openai-public-us-for-eu-pi] :pii_prompt_policy eu-pinned)))
    (cross-region-data-policy
      :default deny
      :allowed-egress-categories
        ((category anonymized-aggregate-metrics :requires [k-anonymity>=50 no-user-id no-prompt-content no-transaction-detail dpo-review])
         (category public-content :requires [no-pii marketing-or-docs-review])
         (category security-threat-intelligence :requires [secops-approval encrypted audited])
         (category compliance-approved-export :requires [legal dpo business-owner export-record data-fingerprint])
         (category code-and-artifacts :requires [no-pii no-config no-secret]))
      :audit (:log central-audit-log :retention "5 years" :cadence quarterly-dpo-review))
	    (project-region-declaration :project pcea
	      :status active-ssot-required
	      :data-regions [cn global]
	      :primary-region global
      :operating-zones [global-us global-eu]
      :contains-personal-data true
      :contains-spi true
	      :contains-important-data unknown
	      :contains-children-data false
	      :cross-region-default deny
	      :platform-partition-binding ((pcea-cn :platform xjp-cn :service-stack [auth-cn secret-store-cn xjp-cn-router xjp-cn-eventhub xjp-cn-ledger])
	                                   (pcea-global :platform xjp-global :service-stack [auth-global secret-store-global xjp-global-router xjp-global-eventhub xjp-global-ledger])
	                                   (pcea-global-eu :platform xjp-global-eu :service-stack [auth-global secret-store-global xjp-global-eu-router xjp-global-eu-eventhub xjp-global-ledger]))
	      :runtime-placement ((pcea-cn :runtime-target ecs-pcea :runtime-provider aliyun-ecs :deploy-center-agent ecs :status current-production-cn-compatible)
	                          (pcea-global :runtime-target gcp-runtime :runtime-provider gcp-vm :deploy-center-agent gcp :status target-pending-provisioning)
	                          (pcea-global-eu :runtime-target gcp-runtime :runtime-provider gcp-vm :deploy-center-agent gcp :status operating-zone-pending-dedicated-eu-runtime))
      :partition-primary-key project_id
      :shared-assets [source-code lisp-blueprints ocaml-compiled-ir rust-binaries container-image-templates anonymized-aggregate-metrics]
      :forbidden [single-instance-multi-region cross-partition-token-trust shared-payment-ledger parent-domain-cookie-sharing ip-only-region-binding]
      :launch-blockers [cn-legal-entity icp-b25 mainland-psp cn-ai-filing important-data-assessment]
      :checker ["node scripts/check-v3-data-residency-universe-isomorphism.mjs" "bash /Users/jinchen/Downloads/PCEA\\ develop/.missiond/check.sh"])
    (project-region-declaration :project cuthub
      :status design-required
	      :data-regions [cn global]
	      :domains ((cn "cuthub.cn") (global "cuthub.com"))
	      :platform-partition-binding ((cuthub-cn :platform xjp-cn :service-stack [auth-cn secret-store-cn xjp-cn-router xjp-cn-eventhub xjp-cn-ledger])
	                                   (cuthub-global :platform xjp-global :service-stack [auth-global secret-store-global xjp-global-router xjp-global-eventhub xjp-global-ledger]))
	      :account-region-binding [explicit-choice phone-country-code payment-method]
      :ip-policy hint-only
      :forbidden [parent-domain-cookie-sharing online-region-switch cn-dot-global-subdomain single-account-dual-skin]
      :next-action "Promote CUTHUB to M6 only after its local SSOT declares the same partition model and checker pins.")
    :surfaces [".missiond/v3/missiond-blueprint.lisp"
               ".missiond/research/data-residency-universe-report-20260512.md"
               "scripts/check-v3-data-residency-universe-isomorphism.mjs"
               "/Users/jinchen/Downloads/PCEA develop/.missiond/intent.lisp"
               "/Users/jinchen/Downloads/PCEA develop/.missiond/check.sh"])

  (deploy-agent-self-update-governance
    :schema "missiond.deploy-agent-self-update-governance.v1"
    :owner deploy-center
    :authority-table deploy_agent_update_provenance
    :facts [agent_id current_version desired_version s3_latest update_status canary_status rollback_marker last_error]
    :events [agent_update_started agent_update_succeeded agent_update_failed rollback_started rollback_succeeded rollback_failed agent_heartbeat agent_offline]
    :rule "deploy-agent self-update and reachability status are deploy-center runtime facts stored in deploy_agent_update_provenance and heartbeat/provenance tables; deploy-center relays update/offline events into MissionD EventBridge so deploy-ops BoardTasks can be triggered from durable events. A failed best-effort notify must not be hidden inside a globally successful release summary: per-agent failure remains actionable until the target reports the desired version or an approved break-glass runbook closes the incident.")

  (project-maturity-model
    :schema "missiond.project-maturity-model.v2"
    :rule "M6 is the highest maturity level and means Auth-grade production-ready SSOT/code/runtime/test clarity: domain model, policy, flow, event, runtime projection, implementation map, compatibility ledger, hot-path wiring, regression matrix, source hygiene, and data-residency declarations for data-bearing projects are fine-grained, code-aligned, and formatter-converged."
    :gate "scripts/check-project-maturity.mjs --min-level M5 is the default universe operational gate; scripts/check-project-maturity.mjs --min-level M6 proves Auth-grade final maturity."
    :levels
      ((level M0 :name raw :requires [] :meaning "unregistered or only scattered facts")
       (level M1 :name registered-intent :requires [project-registration intent-l1-index])
       (level M2 :name blueprint-split :requires [M1 project-blueprint pillar-function-entry-core-egress-surface ordered-steps])
       (level M3 :name code-mapped :requires [M2 code-isomorphism-checker current-code-mapping drift-policy])
       (level M4 :name runtime-projected :requires [M3 runtime-config-from-lisp event-contract deploy-runtime-constants no-hardcoded-runtime-duplicates])
       (level M5 :name worker-operational :requires [M4 mission_swarm_run context-pack-shards scoped-write-guards durable-completion-evidence final-convergence-gate])
       (level M6 :name auth-grade :requires [M5 domain-model policy flow event runtime-projection implementation-map compatibility-ledger hot-path-wiring regression-matrix data-residency-declaration formatter-converged final-m6-report] :meaning "Auth-grade: the project is fine-grained, clear, runtime-wired, region-aware where it carries regulated data, regression-proven, formatter-safe, and safe for long-term dependency."))
    :invariants
      ["Project SSOT reports MUST use only M0..M6; H-levels and M10 are retired public maturity vocabulary."
       "Old M10 maps to new M5 unless the project also has Auth-grade depth evidence."
       "M6 requires Auth-grade domain/policy/flow/event/runtime/implementation/compatibility/hot-path/regression evidence plus formatter convergence: official project formatter checks must be safe to run without unrelated churn. Data-bearing projects also require a data-residency declaration that states region partitions, cross-region defaults, data classes, and compliance blockers."
       "Universe status MUST expose current and target maturity for each registered project."
       "Intent-only projects MUST NOT be marked M2+; projects without code-isomorphism evidence MUST NOT be marked M3+."
       "Resident master and swarm runners MUST use M6 SSOT convergence language and never create H-level tasks."])

  (project-maturity-registry
    :schema "missiond.project-maturity-registry.v2"
    :default-target M6
    :common-m5-to-m6-gap [domain-model policy-flow-event-split compatibility-ledger hot-path-wiring regression-matrix data-residency-declaration final-m6-report]
    (maturity :id missiond :current M6 :target M6 :gap [])
    (maturity :id board :current M5 :target M6 :gap [frontend-domain-model cockpit-hot-path-regressions final-m6-report])
    (maturity :id jarvis :current M5 :target M6 :gap [domain-shard-split missiond-integration-boundary final-m6-report])
    (maturity :id jarvis-forge :current M6 :target M6 :gap [])
    (maturity :id jarvis-mechanic :current M5 :target M6 :gap [mechanic-workflow-boundary missiond-overlap-ledger final-m6-report])
    (maturity :id xjpcode :current M5 :target M6 :gap [domain-shard-split codegen-policy-ledger final-m6-report])
    (maturity :id neural-codegen :current M5 :target M6 :gap [domain-shard-split generation-policy-hot-path final-m6-report])
    (maturity :id semantic-terminal :current M5 :target M6 :gap [domain-shard-split terminal-event-contract final-m6-report])
    (maturity :id xiaojinpro-backend :current M5 :target M6 :gap [monorepo-service-boundary deploy-fact-authority final-m6-report])
    (maturity :id deploy-center :current M6 :target M6 :gap [])
    (maturity :id xjp-memory :current M6 :target M6 :gap [])
    (maturity :id xjp-eventhub :current M6 :target M6 :gap [])
    (maturity :id xjp-mcp :current M5 :target M6 :gap [tool-policy-ledger mcp-permission-regressions final-m6-report])
    (maturity :id xjp-cli :current M5 :target M6 :gap [command-policy-ledger mcp-parity-regressions final-m6-report])
    (maturity :id deploy-agent :current M6 :target M6 :gap [])
    (maturity :id auth :current M6 :target M6 :gap [])
    (maturity :id router :current M6 :target M6 :gap [])
    (maturity :id payments :current M6 :target M6 :gap [])
    (maturity :id asr :current M5 :target M6 :gap [job-provider-transcript-domain callback-regressions final-m6-report])
    (maturity :id timeline :current M5 :target M6 :gap [revision-event-authority service-event-regressions final-m6-report])
    (maturity :id pcea :current M6 :target M6 :gap [])
    (maturity :id xiaojinpro-ios :current M6 :target M6 :gap [])
    (maturity :id secret-store :current M5 :target M6 :gap [secret-version-rotation-domain capability-regressions final-m6-report])
    (maturity :id xiaojin-blog :current M5 :target M6 :gap [content-publishing-domain deploy-auth-boundary final-m6-report])
    (maturity :id cuthub :current M5 :target M6 :gap [community-domain auth-product-dependency final-m6-report])
    (maturity :id legacy-refactor-service :current M5 :target M6 :gap [deep-code-rewrite-worker customer-frontend forge-runtime-provider production-deploy-provenance final-m6-report]))

  (project-blueprint-registry
    :schema "missiond.project-blueprint-registry.v1"
    :rule "Project-local app blueprints are independent SSOT files registered from V3; backend V3 stays compact and aggregate checkers follow the registry pointer."
    (project :id board
      :kind frontend-nextjs
      :path ".missiond/frontend/board-blueprint.lisp"
      :package "packages/board/package.json"
      :status code-aligned
      :checks ["node scripts/check-frontend-board-lisp-schema.mjs"
               "node scripts/check-frontend-board-code-isomorphism.mjs"
               "node scripts/check-frontend-board-runtime-projection.mjs"
               "node scripts/project-frontend-board-config.mjs --check"]
      :surface board-frontend)
    (project :id jarvis-forge
      :kind multi-crate-nextjs
      :root "/Users/jinchen/Projects/jarvis-forge"
      :intent ".missiond/intent.lisp"
      :backend ".missiond/backend/forge-backend-blueprint.lisp"
      :frontend ".missiond/frontend/forge-ui-blueprint.lisp"
      :status project-ssot-owned
      :checks ["bash .missiond/check.sh"]
      :missiond-role "registered project; Lisp/component reuse engine, not MissionD runtime orchestrator"
      :surface project-registry)
    ;; ── Part1 devtools — sibling devtool repos with M5 SSOT, registered as a group ──
    (project :id jarvis
      :kind rust-multi-crate
      :root "/Users/jinchen/Projects/jarvis"
      :intent ".missiond/intent.lisp"
      :backend ".missiond/backend/jarvis-backend-blueprint.lisp"
      :frontend ".missiond/frontend/jarvis-ui-blueprint.lisp"
      :status project-ssot-owned
      :checks ["bash .missiond/check.sh"]
      :missiond-role "registered devtool; clean MissionD rewrite (intent.lisp + 14 intent-*.lisp shards + GAP_ANALYSIS.md)"
      :surface project-registry)
    (project :id jarvis-mechanic
      :kind rust-cli
      :root "/Users/jinchen/Projects/jarvis-mechanic"
      :intent ".missiond/intent.lisp"
      :backend ".missiond/backend/jarvis-mechanic-backend-blueprint.lisp"
      :status project-ssot-owned
      :checks ["node scripts/check-mechanic-ssot.mjs"
               "bash .missiond/check.sh"]
      :missiond-role "registered devtool; opt-in repair executor CLI, not a MissionD orchestrator or automatic runtime worker"
      :surface project-registry)
    (project :id xjpcode
      :kind rust-cli
      :root "/Users/jinchen/Projects/xjpcode"
      :intent ".missiond/intent.lisp"
      :backend ".missiond/xjpcode-app-blueprint.lisp"
      :status project-ssot-owned
      :checks ["node scripts/check-xjpcode-ssot-complete.mjs --json"
               "node scripts/check-xjpcode-code-isomorphism.mjs"]
      :missiond-role "registered devtool; ratatui TUI Rust CLI agent"
      :surface project-registry)
    (project :id neural-codegen
      :kind rust-multi-crate
      :root "/Users/jinchen/Projects/neural-codegen"
      :intent ".missiond/intent.lisp"
      :backend ".missiond/neural-codegen-blueprint.lisp"
      :status project-ssot-owned
      :checks ["bash .missiond/check.sh"]
      :missiond-role "registered devtool; deterministic Lisp→IR→Rust codegen pipeline"
      :surface project-registry)
    (project :id semantic-terminal
      :kind rust-napi-cdylib
      :root "/Users/jinchen/Projects/semantic-terminal"
      :intent ".missiond/intent.lisp"
      :backend ".missiond/semantic-terminal-blueprint.lisp"
      :status project-ssot-owned
      :checks ["bash .missiond/check.sh"]
      :missiond-role "registered devtool; PTY semantic event parser (Rust core + N-API)"
      :surface project-registry)
    (project :id xiaojinpro-backend
      :kind rust-monorepo
      :root "/Users/jinchen/Downloads/xiaojinpro-gateway/xiaojinpro-backend"
      :intent ".missiond/intent.lisp"
      :backend ".missiond/backend/xiaojinpro-backend-blueprint.lisp"
      :status v3-runtime-ssot
      :checks ["node scripts/check-xjp-ssot-complete.mjs"]
      :surface project-registry)
    (project :id xjp-mcp
      :kind node-mcp-server
      :root "/Users/jinchen/Downloads/xiaojinpro-gateway/xiaojinpro-backend/tools/xjp-mcp"
      :intent ".missiond/intent.lisp"
      :backend ".missiond/backend/xjp-mcp-backend-blueprint.lisp"
      :status project-ssot-owned
      :checks ["bash .missiond/check.sh"]
      :missiond-role "registered XJP infra tool surface; ClaudeCode/MissionD-facing MCP bridge for deploy/auth/secret/storage/router operations, not deployment fact authority"
      :surface project-registry)
    (project :id xjp-cli
      :kind rust-cli
      :root "/Users/jinchen/Downloads/xiaojinpro-gateway/xiaojinpro-backend/crates/xjp-cli"
      :intent ".missiond/intent.lisp"
      :backend ".missiond/backend/xjp-cli-backend-blueprint.lisp"
      :status project-ssot-owned
      :checks ["bash .missiond/check.sh"]
      :missiond-role "registered XJP infra operator CLI and embedded MCP server; distinct from apps/xjp-deploy-agent remote execution daemon"
      :surface project-registry)
    (project :id deploy-center
      :aliases [xjp-deploy-center]
      :kind ops-service
      :root "/Users/jinchen/Downloads/xiaojinpro-gateway/xiaojinpro-backend/services/deploy-center"
      :intent ".missiond/intent.lisp"
      :backend ".missiond/backend/deploy-center-backend-blueprint.lisp"
      :status v3-runtime-ssot
      :capability deploy-ops
      :note "xjp-deploy-center is a historical alias for this same canonical service root, not an active Universe project."
      :surface project-registry)
    (project :id deploy-agent
      :aliases [xjp-deploy-agent]
      :kind ops-agent
      :root "/Users/jinchen/Downloads/xiaojinpro-gateway/xiaojinpro-backend/apps/xjp-deploy-agent"
      :intent ".missiond/intent.lisp"
      :backend ".missiond/backend/deploy-agent-backend-blueprint.lisp"
      :status project-ssot-owned
      :capability deploy-ops
      :surface project-registry)
    (project :id auth
      :kind rust-service
      :root "/Users/jinchen/Downloads/xiaojinpro-gateway/xiaojinpro-backend/services/auth"
      :intent ".missiond/intent.lisp"
      :backend ".missiond/backend/auth-backend-blueprint.lisp"
      :status v3-runtime-ssot
      :surface project-registry)
    (project :id router
      :kind rust-service
      :root "/Users/jinchen/Downloads/xiaojinpro-gateway/xiaojinpro-backend/services/router"
      :intent ".missiond/intent.lisp"
      :backend ".missiond/backend/router-backend-blueprint.lisp"
      :status v3-runtime-ssot
      :surface project-registry)
    (project :id xjp-memory
      :kind rust-service
      :root "/Users/jinchen/Downloads/xiaojinpro-gateway/xiaojinpro-backend/services/xjp-memory"
      :intent ".missiond/intent.lisp"
      :backend ".missiond/backend/xjp-memory-backend-blueprint.lisp"
      :status contract-first-service
      :checks ["bash .missiond/check.sh"]
      :missiond-role "registered memory provider service; owns private memory, review overlay, skill evidence, FTS/embedding/rerank storage behind MissionD memory-provider-contract"
      :surface project-registry)
    (project :id xjp-eventhub
      :kind rust-service
      :root "/Users/jinchen/Downloads/xiaojinpro-gateway/xiaojinpro-backend/services/xjp-eventhub"
      :intent ".missiond/intent.lisp"
      :backend ".missiond/backend/xjp-eventhub-backend-blueprint.lisp"
      :status contract-first-service
      :checks ["bash .missiond/check.sh"]
      :missiond-role "registered EventHub service; owns cross-service durable event envelopes while MissionD local EventBus remains offline-safe"
      :surface project-registry)
    (project :id payments
      :kind rust-workspace-service
      :root "/Users/jinchen/Downloads/xiaojinpro-gateway/xiaojinpro-backend/services/payments"
      :intent ".missiond/intent.lisp"
      :backend ".missiond/backend/payments-backend-blueprint.lisp"
      :status v3-runtime-ssot
      :surface project-registry)
    (project :id asr
      :kind rust-service
      :root "/Users/jinchen/Downloads/xiaojinpro-gateway/xiaojinpro-backend/services/asr"
      :intent ".missiond/intent.lisp"
      :backend ".missiond/backend/asr-backend-blueprint.lisp"
      :status v3-runtime-ssot
      :surface project-registry)
    (project :id timeline
      :kind rust-service
      :root "/Users/jinchen/Downloads/xiaojinpro-gateway/xiaojinpro-backend/services/timeline"
      :intent ".missiond/intent.lisp"
      :backend ".missiond/backend/timeline-backend-blueprint.lisp"
      :status v3-runtime-ssot
      :surface project-registry)
    (project :id pcea
      :kind rust-vite-app
      :root "/Users/jinchen/Downloads/PCEA develop"
      :intent ".missiond/intent.lisp"
      :backend ".missiond/backend/pcea-backend-blueprint.lisp"
      :frontend ".missiond/frontend/pcea-frontend-blueprint.lisp"
      :status project-ssot-owned
      :surface project-registry)
    (project :id xiaojinpro-ios
      :kind ios-swiftui-app
      :root "/Users/jinchen/development/xiaojinproIOS/xiaojinpro"
      :intent ".missiond/intent.lisp"
      :frontend ".missiond/frontend/xiaojinpro-ios-blueprint.lisp"
      :operations ".missiond/operations/xiaojinpro-ios-operations-blueprint.lisp"
      :status project-ssot-owned
      :checks ["bash .missiond/check.sh"]
      :missiond-role "registered mobile control client; iPhone entry for Jarvis/MissionD, using Auth JWT and Jarvis HTTPS proxy to control the Mac mini MissionD node"
      :surface project-registry)
    ;; ── App + external-infra projects — already-converged with project-local check.sh runners ──
    (project :id secret-store
      :aliases [secret-store-rs]
      :kind rust-axum-microservice
      :root "/Users/jinchen/Downloads/xiaojinpro-gateway/services/secret-store-rs"
      :intent ".missiond/intent.lisp"
      :status project-ssot-owned
      :lifecycle external-infra-runtime
      :checks ["bash .missiond/check.sh"]
      :missiond-role "registered external infra runtime; AES-256-GCM credential vault (frozen LTS) consumed by auth/deploy-center/* via xjp-config HybridSecretProvider; production endpoint ss.xiaojinpro.top is now on the GCP xjp-backend VM with Caddy proxy to the local secret-store container"
      :surface project-registry)
    (project :id xiaojin-blog
      :kind nextjs-app
      :root "/Users/jinchen/Projects/xiaojin-blog"
      :intent ".missiond/intent.lisp"
      :status project-ssot-owned
      :checks ["bash .missiond/check.sh"]
      :missiond-role "registered app; ruoqijin.com personal blog + research portal (Next.js 16 + React 19 + Drizzle/PG; standalone repo xiaojinpro-team/xiaojin-blog)"
      :surface project-registry)
    (project :id cuthub
      :kind nextjs-app
      :root "/Users/jinchen/Downloads/xiaojinpro-gateway/xiaojinpro-backend/cuthub-frontend"
      :intent ".missiond/intent.lisp"
      :status project-ssot-owned
      :lifecycle canonical-temporary-downloads-checkout
      :note "Supervisor decision 39a2e6e8 — Downloads checkout accepted as temporary canonical M6 SSOT root until repo is cloned to /Users/jinchen/Projects/cuthub-frontend"
      :checks ["bash .missiond/check.sh"]
      :missiond-role "registered app; cuthub.ai frontend (Next.js 16 + React 19 + Tailwind 4 + Konva); independent repo rickyjim626/cuthub-frontend"
      :surface project-registry)
    (project :id legacy-refactor-service
      :kind node-service
      :root "/Users/jinchen/Projects/legacy-refactor-service"
      :intent ".missiond/intent.lisp"
      :backend ".missiond/backend/legacy-refactor-backend-blueprint.lisp"
      :operations ".missiond/deploy/legacy-refactor-deploy-blueprint.lisp"
      :status external-product-service
      :checks ["node scripts/check-legacy-refactor-ssot.mjs --json"]
      :missiond-role "registered external product service; MissionD may orchestrate and observe jobs, while the service owns customer-safe refactor runtime and never exposes internal Lisp/IR/Forge artifacts to customers"
      :surface project-registry))

  (service-runtime-universe
    :schema "missiond.service-runtime-universe.v1"
    :rule "Production service runtime facts are Lisp-owned Universe data: project/service roots, domains, deployments, health, DNS capability, and ops owner are visible to resident master and workers through mission_project(action=universe). Secrets stay outside Lisp."
    (service :id auth
      :project xiaojinpro-backend
      :root "/Users/jinchen/Downloads/xiaojinpro-gateway/xiaojinpro-backend/services/auth"
      :intent ".missiond/intent.lisp"
      :backend ".missiond/backend/auth-backend-blueprint.lisp"
      :environment production
      :public-base-url "https://auth.xiaojinpro.com"
      :issuer "https://auth.xiaojinpro.com"
      :domains ["auth.xiaojinpro.com"]
      :dns-provider cloudflare
      :dns-capability (:read-inventory true :mutate requires-board-approval :secret-source env)
      :deployment (:substrate kubernetes :namespace production :deployment "xjp-auth-center" :service "xjp-auth-center" :replicas 3 :hpa-min 3 :hpa-max 10 :image "xjp-auth-center:latest" :service-account "xjp-auth-center")
      :proxy (:kind caddy :domain "auth.xiaojinpro.com" :file "/Users/jinchen/Downloads/xiaojinpro-gateway/xiaojinpro-backend/services/auth/caddy/Caddyfile" :sse-no-buffer "/auth/login-stream")
      :ports (:http 8081 :metrics 9090 :service 80)
      :health ["/health/live" "/health/ready" "/.well-known/openid-configuration" "/.well-known/jwks.json"]
      :event-ingest (:endpoint "/webhooks/auth-event" :domain system :event ExternalServiceEvent :source auth-audit-events :token-env MISSIOND_EXTERNAL_WEBHOOK_TOKEN :authority provider-durable-log-first :rule "Auth emits sanitized service events into MissionD EventBus via deploy-center adapter; X-MissionD-Webhook-Token is required when MISSIOND_EXTERNAL_WEBHOOK_TOKEN is configured; PTY is diagnostic only and MissionD must not require production probing to observe auth incidents.")
      :dependencies [postgres redis secret-store wechat-open-platform google-oauth sms-provider email-provider]
      :ops-capability deploy-ops
      :source-evidence ["/Users/jinchen/Downloads/xiaojinpro-gateway/xiaojinpro-backend/services/auth/k8s/production/configmap.yaml" "/Users/jinchen/Downloads/xiaojinpro-gateway/xiaojinpro-backend/services/auth/k8s/production/deployment.yaml" "/Users/jinchen/Downloads/xiaojinpro-gateway/xiaojinpro-backend/services/auth/caddy/Caddyfile"]
      :risks [wechat-callback-prod-drift mysql-artifact-cleanup])
    (service :id deploy-center
      :project deploy-center
      :root "/Users/jinchen/Downloads/xiaojinpro-gateway/xiaojinpro-backend/services/deploy-center"
      :intent ".missiond/intent.lisp"
      :backend ".missiond/backend/deploy-center-backend-blueprint.lisp"
      :environment production
      :deployment (:substrate deploy-center :authority release-provenance :provenance-api "/api/deploy/provenance/:project")
      :deployment-confirmation (:checker "node scripts/check-m6-deployment-status.mjs --json" :status-api "/api/deploy/status" :rollout-workflow ".missiond/workflows/m6-deployment-rollout.lisp")
      :event-ingest (:endpoint "/webhooks/deploy-center-event" :domain system :event ExternalServiceEvent :source deploy_events :token-env MISSIOND_EXTERNAL_WEBHOOK_TOKEN :authority deploy-center.deploy_events :rule "deploy-center relays durable deploy_events rows into MissionD EventBridge with stable event_id and MissionD idempotency; MissionD must not infer production release state by stitching GitHub/curl/git when deploy-center has provenance.")
      :events [deploy_created build_started build_succeeded build_failed deploy_started deploy_succeeded deploy_failed smoke_succeeded smoke_failed rollback_started rollback_succeeded rollback_failed agent_heartbeat agent_update_started agent_update_succeeded agent_update_failed provenance_changed]
      :ops-capability deploy-ops
      :surface service-runtime-universe)
    (service :id secret-store
      :project secret-store
      :root "/Users/jinchen/Downloads/xiaojinpro-gateway/services/secret-store-rs"
      :intent ".missiond/intent.lisp"
      :environment production
      :public-base-url "https://ss.xiaojinpro.top"
      :domains ["ss.xiaojinpro.top"]
      :deployment (:substrate gcp-vm :runtime-target gcp-runtime :container "secret-store" :local-bind "127.0.0.1:8091" :proxy caddy :authority deploy-center-provenance)
      :health ["/livez" "/readyz"]
      :dependencies [xjp-postgres secret-store-kek admin-key]
      :ops-capability deploy-ops
      :source-evidence [secret-store-gcp-migration-20260511]
      :surface service-runtime-universe)
    (service :id secret-store-cn
      :project secret-store
      :root "/Users/jinchen/Downloads/xiaojinpro-gateway/services/secret-store-rs"
      :intent ".missiond/intent.lisp"
      :environment cn-production
      :public-base-url "https://ss-cn.xiaojinpro.com"
      :domains ["ss-cn.xiaojinpro.com"]
      :deployment (:substrate aliyun-ecs :dc_slug "secret-store-cn" :runtime-target ecs-pcea :network-profile ecs-cn-restricted :executor ecs-agent :work_dir "/opt/secret-store-cn" :compose_file "/opt/secret-store-cn/docker-compose.cn.yml" :local-bind "127.0.0.1:8091" :proxy nginx :artifact-delivery-lane cn-oss-bundle-lane :authority verified-smoke :deploy-center-status stale-runtime-shell :provenance partial)
      :health ["/livez" "/readyz"]
      :dependencies [cn-postgres secret-store-cn-kek secret-store-cn-admin-key]
      :ops-capability deploy-ops
      :source-evidence [secret-store-cn-ecs-deploy-20260513 secret-store-cn-runtime-verified-20260515 skill:secret-store skill:aliyun]
      :risks [deploy-center-read-model-gap provenance-contract-required docker-healthcheck-disabled-until-next-image-promotion]
      :surface service-runtime-universe)
    (service :id xjp-memory
      :project xjp-memory
      :root "/Users/jinchen/Downloads/xiaojinpro-gateway/xiaojinpro-backend/services/xjp-memory"
      :intent ".missiond/intent.lisp"
      :backend ".missiond/backend/xjp-memory-backend-blueprint.lisp"
      :environment local-dev
      :deployment (:substrate deploy-center :dc_slug "xjp-memory" :container_name "xjp-memory" :default-port 8091 :authority release-provenance)
      :local-runtime (:substrate launchd :label "com.xjp.memory.provider" :url "http://127.0.0.1:8091" :database "xjp_memory" :storage postgres-durable :bringup "scripts/manage-local-providers.sh")
      :health ["/health" "/health/live" "/health/ready" "/v1/memory/provider_status"]
      :dependencies [xjp-router secret-store postgres?]
      :ops-capability memory-provider
      :surface service-runtime-universe)
    (service :id xjp-eventhub
      :project xjp-eventhub
      :root "/Users/jinchen/Downloads/xiaojinpro-gateway/xiaojinpro-backend/services/xjp-eventhub"
      :intent ".missiond/intent.lisp"
      :backend ".missiond/backend/xjp-eventhub-backend-blueprint.lisp"
      :environment local-dev
      :deployment (:substrate deploy-center :dc_slug "xjp-eventhub" :container_name "xjp-eventhub" :default-port 8092 :authority release-provenance)
      :local-runtime (:substrate launchd :label "com.xjp.eventhub.provider" :url "http://127.0.0.1:8092" :database "xjp_eventhub" :storage postgres-durable :bringup "scripts/manage-local-providers.sh")
      :health ["/health" "/health/live" "/health/ready" "/v1/eventhub/status"]
      :dependencies [deploy-center timeline? postgres?]
      :ops-capability eventhub
      :surface service-runtime-universe)
    (service :id legacy-refactor-service
      :project legacy-refactor-service
      :root "/Users/jinchen/Projects/legacy-refactor-service"
      :intent ".missiond/intent.lisp"
      :backend ".missiond/backend/legacy-refactor-backend-blueprint.lisp"
      :environment local-dev
      :public-base-url "http://127.0.0.1:8788"
      :deployment (:substrate local-node :entrypoint "node src/server.mjs" :port-env LEGACY_REFACTOR_PORT :default-port 8788)
      :health ["/health"]
      :dependencies [forge-catalog? missiond-eventbridge?]
      :ops-capability project-refactor
      :surface service-runtime-universe)
    (capability :id cloudflare-dns
      :provider cloudflare
      :default-mode read-only-inventory
      :mutating-policy "Cloudflare DNS mutation requires env/secret binding, deploy-ops capability, and explicit Board approval; workers must report unavailable rather than pretend they can operate DNS when credentials are absent."
      :secrets [CLOUDFLARE_API_TOKEN CLOUDFLARE_ACCOUNT_ID CLOUDFLARE_ZONE_ID]
      :surface service-runtime-universe))
