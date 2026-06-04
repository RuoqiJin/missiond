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

(service-runtime-universe
    :schema "missiond.service-runtime-universe.v1"
    :rule "Production service runtime facts are Lisp-owned Universe data: project/service roots, domains, deployments, health, DNS capability, and ops owner are visible to resident master and workers through mission_project(action=universe). Secrets stay outside Lisp."
    :compiled-support-catalog-fields [support_catalog deploy_center_slug service_manifest_refs health smoke runtime_target db_migration_namespace credential_refs]
    (deployment-channel-plane
      :schema "missiond.deployment-channel-plane.v1"
      :authority-model (:declared missiond-v3-ssot :inferred repo-config-workflows :observed deploy-center-vercel-gcp :secret-values secret-store-only)
      :fields [project_id service_id surface channel_kind authority source_ref workflow deploy_center_slug executor builder runner_role source_sync dockerfile manifest artifact_lane image target_side_build_prohibited declared_status observed_status drift_status]
      :channel-surfaces [build runtime frontend domain self_update]
      :channel-kinds [native_workflow privatecloud_docker_build github_actions deploy_center_runtime gcp_vm vercel kubernetes local_runtime manual_break_glass unknown]
      :scheduler-lanes [control-plane prod-release frontend-vercel frontend-cn bulk-build]
      :executor-readiness-layers [registry-active instance-online instance-ready]
      :vercel-ops-scope [deploy inspect wait promote rollback remove-safe alias-read alias-set webhook-reconcile]
      :canonical-form ":deployment-channels ((channel :surface build ...))"
      :compat-forms [:build-lane :deployment :frontend-deployment]
      :merge-precedence [explicit-v3-deployment-channels project-local-deploy-center-config repo-workflow-inference live-observed-annotation]
      :invariants
        ["MissionD V3 SSOT owns declared deployment-channel intent; deploy-center, GitHub Actions, Vercel, and GCP facts annotate observed state and drift but must not silently overwrite declared channels."
         "Every backend or runtime service must expose exactly one build channel unless explicitly classified frontend-only, local-runtime-only, or manual-break-glass."
         "A runtime deploy-center channel is not evidence that build also runs in deploy-center; build and runtime surfaces must be shown separately."
         "Deploy Center slugs such as xjp-payments/xjp-asr/xjp-deploy-center, container names, and service domains are first-class resolver aliases for service runtime and deployment-channel queries; agents must not rely on conversation search or grep to translate them back to canonical service ids."
         "Native workflow build channels require deploy_project_stage_configs.build.config.deploy_type=native_workflow and target-side build must be prohibited for production Rust product-service backends."
         "Deploy-center may actively manage frontend channels when deploy_project_stage_configs.frontend.config.deploy_type=vercel_frontend; Vercel webhook facts then reconcile deploy-center's managed deployment ledger instead of being only observed external state."
         "Deploy Center native workflow jobs must carry scheduler_lane, priority, concurrency_group, and required_capabilities; control-plane work such as xjp-deploy-center self-update must not be starved behind prod-release or bulk-build work."
         "Deploy Center production jobs must carry a ReleasePlan-derived runner_role. gcp-agent may observe and mutate runtime targets only as runtime_runner, privatecloud 10900kf/12900kf lanes own build_runner work, and macmini may claim only self_update_runner, Darwin, or explicitly declared Vercel/frontend tooling lanes."
         "runner_required_env is a ReleasePlan projection, not an adapter-specific afterthought; backend, cn_frontend, frontend-vercel, and native_workflow paths must all project SecretRequirement.env_name into job metadata before any runner claim."
         "Deploy Center executor health is a three-layer fact: registry active, concrete agent instance online, and instance ready with version, OS/arch, toolchain, workspace, service-manager, and secret-resolution capability evidence."
         "Deploy Center Vercel authority is limited to production deployment lifecycle operations and aliases: deploy/build, inspect/wait, promote, rollback, safe remove, alias read/set, and webhook reconciliation; project deletion, DNS mutation, and secret value management remain outside Deploy Center direct write scope."
         "GitHub Actions build channels must name the workflow and deploy-center trigger slug when present, so project management can answer which services still build through GA without grep."]
      :implementation-surfaces ["scripts/compile-v3-runtime.mjs" "mission_project(action=deployment_channels)" "mission_project(action=reconcile_deployment_channels)" "packages/board/src/app/api/projects/route.ts" "packages/board/src/components/SystemDashboard.tsx"])
    (domain-control-plane
      :schema "missiond.domain-control-plane.v1"
      :authority xjp-domain-service
      :entrypoint "https://domains.xiaojins.com/v1/domains"
      :binding-lifecycle [planned dns_ready proxy_ready smoke_ready active blocked]
      :active-rule "DNS readiness is necessary but not sufficient; production availability is active only after DNS, proxy, and smoke evidence are all ready."
      :desired-state-import "/v1/domains/desired-state/import"
      :dns-json-contract "DnsRecordSpec JSON uses lowercase type values, for example {\"type\":\"a\",\"name\":\"deploy.xiaojins.com\"}; record_type/A payloads are invalid client calls."
      :source-kinds [service-runtime-domains project-registry-aliases project-registry-role-text dns-records frontend-deployment production-domain domain-control-required-binding]
      :providers [cloudflare aliyun]
      :managed-zones ["xiaojins.com" "xiaojinpro.top" "xiaojinpro.com" "speechscribe.top" "wepub.top" "jinstudio.com" "ruoqijin.com" "cuthub.ai" "problemwise.top" "missiond.com" "changtu.pro" "pcea.top"]
      :provider-zones ((cloudflare ["xiaojins.com" "xiaojinpro.top" "speechscribe.top" "wepub.top" "ruoqijin.com" "cuthub.ai" "problemwise.top" "longimage.top" "tiermate.top" "xiaojin.pro"])
                       (aliyun ["xiaojinpro.com" "changtu.pro" "pcea.top"]))
      :required-domains ["files.xiaojins.com"]
      :excluded-domains ["xjp-asr-web.vercel.app" "cname.vercel-dns.com"]
      :mutation-policy approval-required
      :default-mode read-only-inventory
      :checker "node scripts/check-domain-proxy-isomorphism.mjs"
      :agent-prompt "When an agent inspects MissionD project management and the question involves domains, DNS records, Cloudflare, Aliyun DNS, public URLs, certificates, Caddy hostnames, or subdomain ownership, resolve the project in MissionD first, then consult compiled domain_management and xjp-domain-service. Do not infer DNS authority from project aliases or hand-run Cloudflare/Aliyun curl; DNS mutation requires xjp-domain-service apply with explicit approval."
      :invariants
        ["xjp-domain-service is the single authority for owned-zone DNS inventory across Cloudflare and Aliyun, desired-state diff, approval-gated apply, and domain binding audit."
         "MissionD project management may identify project/domain relationships, but it MUST delegate DNS truth and mutation to xjp-domain-service."
         "Domain binding readiness MUST expose DNS, proxy, and smoke dimensions separately; ready means DNS-ready and active means production usable."
         "Deploy Center owns generated GCP Caddy route bundles and smoke evidence; MissionD compiles desired proxy intent but does not hand-edit live Caddyfiles."
         "Owned domains discovered from service-runtime domains, project aliases, project role text, frontend production domains, and DNS records MUST be projected into compiled domain_management unless explicitly excluded."
         "External fallback domains such as xjp-asr-web.vercel.app remain visible as project runtime facts but MUST NOT be treated as owned DNS to manage."
         "files.xiaojins.com is a required XJP media/file binding even while the file service runtime projection is catching up from the XJP backend registry."])
    (service :id auth
      :project xiaojinpro-backend
      :root "/Users/jinchen/Downloads/xiaojinpro-gateway/xiaojinpro-backend/services/auth"
      :intent ".missiond/intent.lisp"
      :backend ".missiond/backend/auth-backend-blueprint.lisp"
      :environment production
      :public-base-url "https://auth.xiaojinpro.com"
      :issuer "https://auth.xiaojinpro.com"
      :canonical-domain "auth.xiaojinpro.com"
      :domain-exception-reason "WeChat callback registration is locked to auth.xiaojinpro.com; auth.xiaojins.com must not be generated."
      :domains ["auth.xiaojinpro.com"]
      :dns-provider cloudflare
      :dns-capability (:read-inventory true :mutate requires-board-approval :secret-source env)
      :deployment (:substrate kubernetes :namespace production :deployment "xjp-auth-center" :service "xjp-auth-center" :replicas 3 :hpa-min 3 :hpa-max 10 :image "xjp-auth-center:latest" :service-account "xjp-auth-center")
      :build-lane (:id privatecloud-rust-build-lane :builder privatecloud-10900kf :executor privatecloud-agent :source-sync deploy-center-codebase :dockerfile "services/auth/Dockerfile" :image "ghcr.io/xiaojinpro-team/xjp-auth" :artifact-lane cloud-registry-lane :manifest "services/auth/service.manifest.toml" :authority deploy-center :target-side-build-prohibited true)
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
      :public-base-url "https://deploy.xiaojins.com"
      :domains ["deploy.xiaojins.com"]
      :dns-provider xjp-domain-service
      :dns-record (:type A :name "deploy.xiaojins.com" :content "34.104.147.118" :proxied false :ttl 60 :authority xjp-domain-service)
      :deployment (:substrate deploy-center :dc_slug "xjp-deploy-center" :runtime-target gcp-runtime :executor gcp-agent :container "xjp-deploy-center" :default-port 8090 :authority release-provenance :provenance-api "/api/deploy/provenance/:project")
      :deployment-confirmation (:checker "node scripts/check-m6-deployment-status.mjs --json" :status-api "/api/deploy/status" :rollout-workflow ".missiond/workflows/m6-deployment-rollout.lisp")
      :event-ingest (:endpoint "/webhooks/deploy-center-event" :domain system :event ExternalServiceEvent :source deploy_events :token-env MISSIOND_EXTERNAL_WEBHOOK_TOKEN :authority deploy-center.deploy_events :rule "deploy-center relays durable deploy_events rows into MissionD EventBridge with stable event_id and MissionD idempotency; MissionD must not infer production release state by stitching GitHub/curl/git when deploy-center has provenance.")
      :events [deploy_created build_started build_succeeded build_failed deploy_started deploy_succeeded deploy_failed workflow_run_created workflow_job_started workflow_job_succeeded workflow_job_failed workflow_job_cancelled workflow_job_lease_expired artifact_recorded smoke_succeeded smoke_failed rollback_started rollback_succeeded rollback_failed agent_heartbeat agent_update_started agent_update_succeeded agent_update_failed provenance_changed closure_verdict]
      :build-lane (:id privatecloud-rust-build-lane :builder privatecloud-10900kf :executor privatecloud-agent :source-sync deploy-center-codebase :dockerfile "docker/Dockerfile.deploy-center" :image "ghcr.io/xiaojinpro-team/xjp-deploy-center" :artifact-lane cloud-registry-lane :manifest "services/deploy-center/service.manifest.toml" :authority deploy-center :target-side-build-prohibited true)
      :proxy (:kind caddy :domain "deploy.xiaojins.com" :routes ["/api/deploy/*" "/mesh/*" "/health" "/health/*"] :compat-domain "auth.xiaojinpro.com" :compat-routes ["/api/deploy/*" "/mesh/*"] :upstream "localhost:8090")
      :health ["/health/live" "/health/ready" "/api/deploy/health" "/api/deploy/healthz/db"]
      :dependencies [xjp-pg-prod secret-store deploy-agent privatecloud-10900kf ghcr]
      :ops-capability deploy-ops
      :source-evidence [services/deploy-center/service.manifest.toml services.yaml deploy/gcp-vm/xjp-postgres-stack/docker-compose.yml]
      :surface service-runtime-universe)
    (service :id missiond-jarvis-edge
      :project missiond
      :root "/Users/jinchen/Projects/missiond"
      :environment production
      :public-base-url "https://jarvis.xiaojins.com"
      :domains ["jarvis.xiaojins.com"]
      :dns-provider xjp-domain-service
      :dns-record (:type A :name "jarvis.xiaojins.com" :content "34.104.147.118" :proxied false :ttl 60 :authority xjp-domain-service)
      :deployment (:substrate gcp-caddy-edge :runtime-target gcp-runtime :origin "104.194.81.38:9876" :tunnel-client "rickyhqmac-mini-jarvis" :target-service missiond :authority verified-smoke)
      :proxy (:kind caddy :domain "jarvis.xiaojins.com" :routes ["/health" "/v1/*" "/api/monitor/jarvis" "/api/readiness" "/jarvis/*"] :compat-domain "jarvis.xiaojinpro.top" :upstream "104.194.81.38:9876" :sse-no-buffer true :flush-interval "-1" :read-timeout "75s" :write-timeout "75s" :stream-timeout "0" :route-generation "jarvis-gcp-bwg-macmini-20260604")
      :jarvis-runtime-topology (:schema "missiond.jarvis-runtime-topology.v1"
        :edge-node gcp-caddy-edge
        :edge-domain "jarvis.xiaojins.com"
        :edge-public-ip "34.104.147.118"
        :edge-proxy caddy
        :origin-node bwg-tunnel
        :origin "104.194.81.38:9876"
        :tunnel-server-url "ws://104.194.81.38:9876/tunnel/ws"
        :tunnel-client-id "rickyhqmac-mini-jarvis"
        :target-node rickyhq-macmini-m4
        :target-service missiond
        :target-local-url "http://127.0.0.1:9120"
        :expected-deploy-agent-version "10.7.15"
        :launchd-unit "com.xiaojinpro.jarvis-tunnel"
        :launchd-plist "~/Library/LaunchAgents/com.xiaojinpro.jarvis-tunnel.plist"
        :local-health-url "http://127.0.0.1:9880/health"
        :route-generation "jarvis-gcp-bwg-macmini-20260604"
        :proxy-no-buffer true
        :proxy-flush-interval "-1"
        :proxy-read-timeout "75s"
        :proxy-write-timeout "75s"
        :proxy-stream-timeout "0"
        :streaming-policy "sse-no-buffer bounded-upstream-idle typed-terminal-diagnostic"
        :authority verified-smoke)
      :ports (:https 443)
      :health ["/health" "/api/readiness" "/api/monitor/jarvis" "/jarvis/api/monitor/jarvis"]
      :dependencies [gcp-runtime caddy cloudflare-dns bwg-tunnel rickyhq-macmini-m4 missiond-daemon]
      :ops-capability deploy-ops
      :source-evidence [jarvis-xiaojins-com-domain-migration-20260604 gcp-caddy-jarvis-edge-20260528 missiond-jarvis-sse-smoke-20260528]
      :risks [dns-local-cache-propagation]
      :surface service-runtime-universe)
    (service :id search-center
      :project search-center
      :root "/Users/jinchen/Downloads/xiaojinpro-gateway/xiaojinpro-backend/services/search-center"
      :intent ".missiond/intent.lisp"
      :backend ".missiond/backend/search-center-backend-blueprint.lisp"
      :environment production
      :public-base-url "https://search-center.xiaojins.com"
      :frontend-url "https://search.xiaojinpro.top"
      :domains ["search.xiaojinpro.top" "search-center.xiaojins.com"]
      :compat-domains ["search-center.xiaojinpro.top" "auth.xiaojinpro.com"]
      :dns-provider xjp-domain-service
      :dns-records [(:type CNAME :name "search.xiaojinpro.top" :content "cname.vercel-dns.com" :proxied false :ttl 60 :authority cloudflare)
                    (:type A :name "search-center.xiaojins.com" :content "34.104.147.118" :proxied false :ttl 60 :authority xjp-domain-service)]
      :deployment (:substrate deploy-center :dc_slug "xjp-search-center" :runtime-target gcp-runtime :executor gcp-agent :container "xjp-search-center" :default-port 3120 :authority release-provenance)
      :build-lane (:id privatecloud-rust-build-lane :builder privatecloud-10900kf :executor privatecloud-agent :source-sync deploy-center-codebase :dockerfile "services/search-center/Dockerfile" :image "ghcr.io/xiaojinpro-team/xjp-search-center" :artifact-lane cloud-registry-lane :manifest "services/search-center/service.manifest.toml" :authority deploy-center :target-side-build-prohibited true)
      :frontend-deployment (:substrate deploy-center :channel_kind vercel :deploy_type vercel_frontend :dc_slug "xjp-search-center" :stage frontend :stage_project_slug "xjp-search-center-web" :executor macmini :vercel_project "xjp-search-center-web" :root "apps/xjp-search-center-web" :production-domain "https://search.xiaojinpro.top" :manual-break-glass "apps/xjp-search-center-web/scripts/deploy-vercel.sh" :authority deploy-center)
      :deployment-channels ((channel :surface build :channel_kind native_workflow :authority deploy-center :deploy_center_slug "xjp-search-center" :executor privatecloud-agent :builder privatecloud-10900kf :source_sync deploy-center-codebase :dockerfile "services/search-center/Dockerfile" :manifest "services/search-center/service.manifest.toml" :artifact_lane cloud-registry-lane :image "ghcr.io/xiaojinpro-team/xjp-search-center" :target_side_build_prohibited true :declared_status active)
                            (channel :surface runtime :channel_kind deploy_center_runtime :authority deploy-center :deploy_center_slug "xjp-search-center" :executor gcp-agent :runtime_target gcp-runtime :manifest "services/search-center/service.manifest.toml" :target_side_build_prohibited true :declared_status active)
                            (channel :surface frontend :channel_kind vercel :authority deploy-center :deploy_center_slug "xjp-search-center" :executor macmini :workflow deploy-center-native-vercel :deploy_type vercel_frontend :stage frontend :stage_project_slug "xjp-search-center-web" :vercel_project "xjp-search-center-web" :root_directory "apps/xjp-search-center-web" :production_domain "https://search.xiaojinpro.top" :manifest "apps/xjp-search-center-web/service.manifest.toml" :source_ref "apps/xjp-search-center-web/deploy/deploy-center/project.json" :declared_status active))
      :proxy (:kind caddy :domain "search-center.xiaojins.com" :routes ["/health/live" "/health/ready" "/v1/health" "/v1/me" "/v1/search" "/v1/search/*" "/v1/research" "/v1/research/*" "/v1/history" "/v1/history/*"] :compat-domain "auth.xiaojinpro.com" :compat-routes ["/v1/search" "/v1/search/*" "/v1/research" "/v1/research/*" "/v1/history" "/v1/history/*"] :upstream "localhost:3120")
      :ports (:http 3120)
      :health ["/v1/health"]
      :dependencies [xjp-auth xjp-payments xjp-router xjp-pg-prod secret-store missiond-jarvis-edge anysearch bocha tavily? brave? dataforseo? exa?]
      :billing (:authority xjp-payments :credit-guard before-provider-call :spend-endpoint "/payments/internal/credits/spend" :required-secret INTERNAL_API_TOKEN :quick-search-cost-credits 1 :deep-research-cost-credits 300)
      :llm-provider (:synthesis-channel missiond-xjpcode-text-only :authority missiond-jarvis-edge :provider claude_code :model claude-opus-4-6 :global-enable-env SEARCH_CENTER_LLM_PROVIDER :json-enable-env SEARCH_CENTER_JSON_PROVIDER :synthesis-enable-env SEARCH_CENTER_SYNTHESIS_PROVIDER :endpoint-env SEARCH_CENTER_MISSIOND_TEXT_ONLY_URL :migration-target provider-interaction-box :rule "Search Center retrieval remains xjp-router/provider fan-out. Deep Research JSON planning/repair and long-form synthesis currently may route through MissionD/xjpcode text-only ClaudeCode on the managed Mac node when SEARCH_CENTER_LLM_PROVIDER=missiond_text_only, or by narrower SEARCH_CENTER_JSON_PROVIDER / SEARCH_CENTER_SYNTHESIS_PROVIDER switches; this text-only lane is migration-only and must become a MissionD provider-interaction-box HTTP adapter backed by interactive PTY plus durable provider-log final extraction. The lane must stay no-tools/no-MCP/no-filesystem and must be protected by internal service auth or a tunnel, never exposed as an unauthenticated public provider.")
      :ops-capability deploy-ops
      :source-evidence [skill:search-center skill:services/search-center]
      :risks [deep-research-live-300-source-artifact-pending cross-domain-benchmark-suite-pending production-browser-oauth-flow-pending provider-secret-activation-pending final-promotion-artifact-verification-pending]
      :surface service-runtime-universe)
    (service :id payments
      :project payments
      :root "/Users/jinchen/Downloads/xiaojinpro-gateway/xiaojinpro-backend/services/payments"
      :intent ".missiond/intent.lisp"
      :backend ".missiond/backend/payments-backend-blueprint.lisp"
      :environment production
      :public-base-url "https://pay.xiaojins.com"
      :api-base-url "https://pay.xiaojins.com/payments"
      :domains ["pay.xiaojins.com"]
      :compat-domains ["pay.xiaojinpro.com" "auth.xiaojinpro.com"]
      :dns-provider xjp-domain-service
      :dns-record (:type A :name "pay.xiaojins.com" :content "34.104.147.118" :proxied false :ttl 60 :authority xjp-domain-service)
      :deployment (:substrate deploy-center :dc_slug "xjp-payments" :runtime-target gcp-runtime :executor gcp-agent :container "xjp-payments" :default-port 8080 :host-bind "127.0.0.1:8080" :authority release-provenance)
      :build-lane (:id privatecloud-rust-build-lane :builder privatecloud-10900kf :executor privatecloud-agent :source-sync deploy-center-codebase :dockerfile "services/payments/Dockerfile" :image "ghcr.io/xiaojinpro-team/xjp-payments" :artifact-lane cloud-registry-lane :manifest "services/payments/service.manifest.toml" :authority deploy-center :target-side-build-prohibited true)
      :proxy (:kind caddy :domain "pay.xiaojins.com" :routes ["/checkout" "/checkout/*" "/health" "/health/*" "/payments" "/payments/*"] :compat-domain "auth.xiaojinpro.com" :compat-routes ["/payments" "/payments/*"] :upstream "localhost:8080")
      :ports (:host 8080 :container 8080)
      :health ["/health" "/payments/health" "/payments/health/ready" "/payments/health/runtime"]
      :dependencies [xjp-auth xjp-router xjp-pg-prod secret-store stripe wechatpay alipay caddy xjp-domain-service aliyun-dns]
      :ops-capability deploy-ops
      :source-evidence [skill:services/payments services/payments/service.manifest.toml payments-provenance-20260602]
      :risks [stripe-live-secret-pending activation-code-test-channel-required multi-product-payment-config-readiness pay-checkout-runtime-release-pending]
      :surface service-runtime-universe)
    (service :id router
      :project router
      :root "/Users/jinchen/Downloads/xiaojinpro-gateway/xiaojinpro-backend/services/router"
      :intent ".missiond/intent.lisp"
      :backend ".missiond/backend/router-backend-blueprint.lisp"
      :environment production
      :api-base-url "https://router.xiaojins.com"
      :domains ["router.xiaojins.com"]
      :compat-domains ["auth.xiaojinpro.com"]
      :dns-provider xjp-domain-service
      :dns-record (:type A :name "router.xiaojins.com" :content "34.104.147.118" :proxied false :ttl 60 :authority xjp-domain-service)
      :deployment (:substrate deploy-center :dc_slug "xjp-router" :runtime-target gcp-runtime :executor gcp-agent :container "xjp-router" :default-port 8080 :host-bind "127.0.0.1:8082" :authority release-provenance)
      :build-lane (:id privatecloud-rust-build-lane :builder privatecloud-10900kf :executor privatecloud-agent :source-sync deploy-center-codebase :dockerfile "services/router/Dockerfile" :image "ghcr.io/xiaojinpro-team/xjp-router" :artifact-lane cloud-registry-lane :authority deploy-center :target-side-build-prohibited true)
      :proxy (:kind caddy :domain "router.xiaojins.com" :routes ["/health/live" "/health/ready" "/healthz" "/v1/chat/*" "/v1/messages" "/v1/models" "/v1/models/*" "/v1/workflows/*" "/v1/community/*" "/v1/embeddings" "/v1/rerank" "/embed" "/rerank"] :compat-domain "auth.xiaojinpro.com" :upstream "localhost:8082")
      :ports (:host 8082 :container 8080)
      :health ["/health/live" "/health/ready" "/healthz" "/v1/models"]
      :dependencies [xjp-auth xjp-pg-prod secret-store xjp-payments xjp-eventhub?]
      :ops-capability deploy-ops
      :source-evidence [services.yaml infra/caddy/Caddyfile]
      :surface service-runtime-universe)
    (service :id timeline
      :project timeline
      :root "/Users/jinchen/Downloads/xiaojinpro-gateway/xiaojinpro-backend/services/timeline"
      :intent ".missiond/intent.lisp"
      :backend ".missiond/backend/timeline-backend-blueprint.lisp"
      :environment production
      :api-base-url "https://timeline.xiaojins.com"
      :domains ["timeline.xiaojins.com"]
      :compat-domains ["auth.xiaojinpro.com"]
      :dns-provider xjp-domain-service
      :dns-record (:type A :name "timeline.xiaojins.com" :content "34.104.147.118" :proxied false :ttl 60 :authority xjp-domain-service)
      :deployment (:substrate deploy-center :dc_slug "xjp-timeline" :runtime-target gcp-runtime :executor gcp-agent :container "xjp-timeline" :default-port 8083 :host-bind "127.0.0.1:8083" :authority release-provenance)
      :proxy (:kind caddy :domain "timeline.xiaojins.com" :routes ["/health" "/health/*" "/v1/timeline/*" "/v1/agent/*" "/v1/dev/*" "/v1/experience/*" "/v1/media/*"] :compat-domain "auth.xiaojinpro.com" :upstream "localhost:8083")
      :ports (:host 8083 :container 8083)
      :health ["/health" "/health/live" "/health/ready"]
      :dependencies [xjp-auth xjp-router xjp-pg-prod redis?]
      :ops-capability deploy-ops
      :source-evidence [services.yaml infra/caddy/Caddyfile]
      :surface service-runtime-universe)
    (service :id transfer
      :project transfer
      :root "/Users/jinchen/Downloads/xiaojinpro-gateway/xiaojinpro-backend/services/transfer"
      :environment production
      :api-base-url "https://transfer.xiaojins.com"
      :domains ["transfer.xiaojins.com"]
      :compat-domains ["auth.xiaojinpro.com"]
      :dns-provider xjp-domain-service
      :dns-record (:type A :name "transfer.xiaojins.com" :content "34.104.147.118" :proxied false :ttl 60 :authority xjp-domain-service)
      :deployment (:substrate deploy-center :dc_slug "xjp-transfer" :runtime-target gcp-runtime :executor gcp-agent :container "xjp-transfer" :default-port 8084 :host-bind "127.0.0.1:8084" :authority release-provenance)
      :proxy (:kind caddy :domain "transfer.xiaojins.com" :routes ["/health" "/health/*" "/v1/transfer/*"] :compat-domain "auth.xiaojinpro.com" :upstream "localhost:8084")
      :ports (:host 8084 :container 8084)
      :health ["/health" "/health/live" "/health/ready"]
      :dependencies [xjp-auth xjp-pg-prod]
      :ops-capability deploy-ops
      :source-evidence [services.yaml infra/caddy/Caddyfile]
      :surface service-runtime-universe)
    (service :id knowledge
      :project knowledge
      :root "/Users/jinchen/Downloads/xiaojinpro-gateway/xiaojinpro-backend/services/knowledge"
      :environment production
      :api-base-url "https://knowledge.xiaojins.com"
      :domains ["knowledge.xiaojins.com"]
      :compat-domains ["auth.xiaojinpro.com"]
      :dns-provider xjp-domain-service
      :dns-record (:type A :name "knowledge.xiaojins.com" :content "34.104.147.118" :proxied false :ttl 60 :authority xjp-domain-service)
      :deployment (:substrate deploy-center :dc_slug "xjp-knowledge" :runtime-target gcp-runtime :executor gcp-agent :container "xjp-knowledge" :default-port 8085 :host-bind "127.0.0.1:8085" :authority release-provenance)
      :proxy (:kind caddy :domain "knowledge.xiaojins.com" :routes ["/health" "/health/*" "/v1/knowledge/*"] :compat-domain "auth.xiaojinpro.com" :upstream "localhost:8085")
      :ports (:host 8085 :container 8085)
      :health ["/health" "/health/live" "/health/ready"]
      :dependencies [xjp-auth xjp-router xjp-pg-prod vector-db?]
      :ops-capability deploy-ops
      :source-evidence [services.yaml infra/caddy/Caddyfile]
      :surface service-runtime-universe)
    (service :id assistant
      :project assistant
      :root "/Users/jinchen/Downloads/xiaojinpro-gateway/xiaojinpro-backend/services/assistant"
      :environment production
      :api-base-url "https://assistant.xiaojins.com"
      :domains ["assistant.xiaojins.com"]
      :compat-domains ["auth.xiaojinpro.com"]
      :dns-provider xjp-domain-service
      :dns-record (:type A :name "assistant.xiaojins.com" :content "34.104.147.118" :proxied false :ttl 60 :authority xjp-domain-service)
      :deployment (:substrate deploy-center :dc_slug "xjp-assistant" :runtime-target gcp-runtime :executor gcp-agent :container "xjp-assistant" :default-port 8086 :host-bind "127.0.0.1:8086" :authority release-provenance)
      :proxy (:kind caddy :domain "assistant.xiaojins.com" :routes ["/health" "/health/*" "/v1/assistant/*"] :compat-domain "auth.xiaojinpro.com" :upstream "localhost:8086")
      :ports (:host 8086 :container 8086)
      :health ["/health" "/health/live" "/health/ready"]
      :dependencies [xjp-auth xjp-router timeline knowledge xjp-pg-prod]
      :ops-capability deploy-ops
      :source-evidence [services.yaml infra/caddy/Caddyfile]
      :surface service-runtime-universe)
    (service :id investor-panel
      :project investor-panel
      :root "/Users/jinchen/Downloads/xiaojinpro-gateway/xiaojinpro-backend/services/investor-panel"
      :environment production
      :api-base-url "https://investor.xiaojins.com/api/investor"
      :domains ["investor.xiaojins.com"]
      :compat-domains ["auth.xiaojinpro.com"]
      :dns-provider xjp-domain-service
      :dns-record (:type A :name "investor.xiaojins.com" :content "34.104.147.118" :proxied false :ttl 60 :authority xjp-domain-service)
      :deployment (:substrate deploy-center :dc_slug "xjp-investor-panel" :runtime-target gcp-runtime :executor gcp-agent :container "xjp-investor-panel" :default-port 8088 :host-bind "127.0.0.1:8088" :authority release-provenance)
      :proxy (:kind caddy :domain "investor.xiaojins.com" :routes ["/health" "/health/*" "/api/investor/*"] :compat-domain "auth.xiaojinpro.com" :upstream "localhost:8088")
      :ports (:host 8088 :container 8088)
      :health ["/health" "/health/live" "/health/ready" "/api/investor/health"]
      :dependencies [xjp-auth xjp-router deploy-center xjp-pg-prod]
      :ops-capability deploy-ops
      :source-evidence [services.yaml infra/caddy/Caddyfile]
      :surface service-runtime-universe)
    (service :id xiaojinpro-frontend
      :project xiaojinpro-frontend
      :root "/Users/jinchen/Downloads/xiaojinpro-gateway/xiaojinpro-backend/xiaojinpro-frontend"
      :intent ".missiond/intent.lisp"
      :frontend ".missiond/frontend/xiaojinpro-frontend-blueprint.lisp"
      :environment production
      :public-base-url "https://xiaojinpro.top"
      :domains ["xiaojinpro.top" "www.xiaojinpro.top"]
      :dns-provider cloudflare
      :deployment (:substrate vercel :project "xiaojinpro-frontend" :framework nextjs :authority release-provenance)
      :health ["/"]
      :dependencies [xjp-auth xjp-router deploy-center object-storage supabase missiond-jarvis-edge]
      :ops-capability deploy-ops
      :source-evidence [xiaojinpro-frontend-project-ssot]
      :risks [next-build-ignores-eslint next-build-ignores-typescript auth-flow-regression-proof-pending]
      :surface service-runtime-universe)
    (service :id asr
      :project asr
      :root "/Users/jinchen/Downloads/xiaojinpro-gateway/xiaojinpro-backend/services/asr"
      :intent ".missiond/intent.lisp"
      :backend ".missiond/backend/asr-backend-blueprint.lisp"
      :frontend ".missiond/frontend/asr-web-blueprint.lisp"
      :operations ".missiond/operations/asr-operations-blueprint.lisp"
      :environment production
      :public-base-url "https://speechscribe.top"
      :frontend-url "https://speechscribe.top"
      :api-base-url "https://asr.xiaojins.com"
      :domains ["speechscribe.top" "www.speechscribe.top" "asr.xiaojins.com" "xjp-asr-web.vercel.app"]
      :compat-domains ["asr.xiaojinpro.top" "auth.xiaojinpro.com"]
      :dns-provider xjp-domain-service
      :dns-records [(:type A :name "speechscribe.top" :content "76.76.21.21" :proxied false :authority cloudflare)
                    (:type CNAME :name "www.speechscribe.top" :content "cname.vercel-dns.com" :proxied false :authority cloudflare)
                    (:type A :name "asr.xiaojins.com" :content "34.104.147.118" :proxied false :ttl 60 :authority xjp-domain-service)]
      :deployment (:substrate deploy-center :dc_slug "xjp-asr" :runtime-target gcp-runtime :executor gcp-agent :container "xjp-asr" :default-port 8090 :host-bind "127.0.0.1:8089" :artifact-delivery-lane cloud-registry-lane :target-side-build-prohibited true :authority release-provenance)
      :build-lane (:id privatecloud-rust-build-lane :builder privatecloud-10900kf :executor privatecloud-agent :runner_agent_id privatecloud :source-sync deploy-center-codebase :dockerfile "services/asr/Dockerfile" :image "ghcr.io/ruoqijin/xjp-asr" :artifact-lane cloud-registry-lane :authority deploy-center :target-side-build-prohibited true)
      :frontend-deployment (:substrate vercel :project "rickyjim626s-projects/xjp-asr-web" :production-domain "speechscribe.top" :fallback-domain "xjp-asr-web.vercel.app")
      :proxy (:kind caddy :domain "asr.xiaojins.com" :routes ["/health/live" "/health/ready" "/api/asr/*" "/v1/*"] :compat-domain "auth.xiaojinpro.com" :compat-routes ["/asr" "/asr/*"] :upstream "localhost:8089")
      :ports (:host 8089 :container 8090)
      :health ["/health/live" "/health/ready" "/api/asr/health"]
      :auth (:provider xjp-auth :client_id "xjp-asr" :redirect_uri "https://speechscribe.top/auth/callback")
      :dependencies [xjp-auth payments xjp-pg-prod redis secret-store volcengine-seed-asr cloudflare-r2 aliyun-oss vercel cloudflare]
      :ops-capability deploy-ops
      :source-evidence [skill:services/asr asr-web-vercel-20260528 cloudflare-dns-asr-20260528]
      :risks [full-browser-oauth-callback-smoke-pending provider-cost-quota-regression-pending]
      :surface service-runtime-universe)
    (service :id xjp-image-service
      :project xjp-image-service
      :root "/Users/jinchen/Downloads/xiaojinpro-gateway/xiaojinpro-backend/services/image"
      :intent ".missiond/intent.lisp"
      :backend ".missiond/backend/xjp-image-service-blueprint.lisp"
      :environment production
      :api-base-url "https://images.xiaojins.com/v1/images"
      :domains ["images.xiaojins.com"]
      :dns-provider cloudflare
      :dns-record (:type A :name "images.xiaojins.com" :content "34.104.147.118" :proxied false :ttl 60 :authority xjp-domain-service)
      :deployment (:substrate deploy-center :dc_slug "xjp-image-service" :runtime-target gcp-runtime :executor gcp-agent :container "xjp-image-service" :default-port 8095 :authority release-provenance)
      :build-lane (:id privatecloud-rust-build-lane :builder privatecloud-10900kf :executor privatecloud-agent :source-sync deploy-center-codebase :dockerfile "services/image/Dockerfile" :image "ghcr.io/xiaojinpro-team/xjp-image-service" :artifact-lane cloud-registry-lane :authority deploy-center :target-side-build-prohibited true)
      :proxy (:kind caddy :domain "images.xiaojins.com" :routes ["/health" "/health/*" "/v1/images" "/v1/images/*"] :upstream "localhost:8095")
      :ports (:http 8095)
      :health ["/health/live" "/health/ready" "/v1/images/uploads/presign"]
      :media-policy (:partition xjp-global :visibility-default private-signed-service-url :public-mode explicit-publish :signed-url-domain "images.xiaojins.com" :variants [original thumbnail preview])
      :dependencies [xjp-auth xjp-pg-prod secret-store object-storage cloudflare-r2 xjp-eventhub? xjp-domain-service]
      :ops-capability deploy-ops
      :source-evidence [xjp-media-service-plan-20260602 xjp-domain-service-media-subdomains-20260602]
      :surface service-runtime-universe)
    (service :id xjp-video-service
      :project xjp-video-service
      :root "/Users/jinchen/Downloads/xiaojinpro-gateway/xiaojinpro-backend/services/video"
      :intent ".missiond/intent.lisp"
      :backend ".missiond/backend/xjp-video-service-blueprint.lisp"
      :environment production
      :api-base-url "https://videos.xiaojins.com/v1/videos"
      :domains ["videos.xiaojins.com"]
      :dns-provider cloudflare
      :dns-record (:type A :name "videos.xiaojins.com" :content "34.104.147.118" :proxied false :ttl 60 :authority xjp-domain-service)
      :deployment (:substrate deploy-center :dc_slug "xjp-video-service" :runtime-target gcp-runtime :executor gcp-agent :container "xjp-video-service" :default-port 8096 :authority release-provenance)
      :build-lane (:id privatecloud-rust-build-lane :builder privatecloud-10900kf :executor privatecloud-agent :source-sync deploy-center-codebase :dockerfile "services/video/Dockerfile" :image "ghcr.io/xiaojinpro-team/xjp-video-service" :artifact-lane cloud-registry-lane :authority deploy-center :target-side-build-prohibited true)
      :runner (:kind deploy-agent-hosted-service :project xjp-video-transcode-runner :binary "xjp-video-transcode-runner" :runtime-target windows-12900kf :agent_url windows :transport self-built-proxy-deploy-program :queue video_jobs :ffmpeg cpu-required :profiles [poster_jpeg mp4_passthrough hls_720p])
      :proxy (:kind caddy :domain "videos.xiaojins.com" :routes ["/health" "/health/*" "/v1/videos" "/v1/videos/*"] :upstream "localhost:8096")
      :ports (:http 8096)
      :health ["/health/live" "/health/ready"]
      :media-policy (:partition xjp-global :visibility-default private-signed-service-url :public-mode explicit-publish :signed-url-domain "videos.xiaojins.com" :transcode-baseline [poster_jpeg mp4_passthrough hls_720p])
      :dependencies [xjp-auth xjp-pg-prod secret-store object-storage cloudflare-r2 ffmpeg xjp-eventhub? xjp-domain-service deploy-agent windows-12900kf]
      :ops-capability deploy-ops
      :source-evidence [xjp-media-service-plan-20260602 xjp-domain-service-media-subdomains-20260602]
      :surface service-runtime-universe)
    (service :id xjp-domain-service
      :project xjp-domain-service
      :root "/Users/jinchen/Downloads/xiaojinpro-gateway/xiaojinpro-backend/services/domain"
      :intent ".missiond/intent.lisp"
      :backend ".missiond/backend/xjp-domain-service-blueprint.lisp"
      :environment production
      :api-base-url "https://domains.xiaojins.com/v1/domains"
      :domains ["domains.xiaojins.com"]
      :dns-provider cloudflare
      :dns-record (:type A :name "domains.xiaojins.com" :content "34.104.147.118" :proxied false :ttl 60 :authority xjp-domain-service)
      :deployment (:substrate deploy-center :dc_slug "xjp-domain-service" :runtime-target gcp-runtime :executor gcp-agent :container "xjp-domain-service" :default-port 8097 :authority release-provenance)
      :build-lane (:id privatecloud-rust-build-lane :builder privatecloud-10900kf :executor privatecloud-agent :source-sync deploy-center-codebase :dockerfile "services/domain/Dockerfile" :image "ghcr.io/xiaojinpro-team/xjp-domain-service" :artifact-lane cloud-registry-lane :authority deploy-center :target-side-build-prohibited true)
      :proxy (:kind caddy :domain "domains.xiaojins.com" :routes ["/health" "/health/*" "/v1/domains" "/v1/domains/*"] :upstream "localhost:8097")
      :ports (:http 8097)
      :health ["/health/live" "/health/ready"]
      :domain-policy (:zone "xiaojins.com" :provider cloudflare :mutation approval-required :apply-authority xjp-domain-service :audit-table domain_apply_audits)
      :dependencies [xjp-auth xjp-pg-prod secret-store cloudflare-dns deploy-center]
      :ops-capability deploy-ops
      :source-evidence [xjp-domain-service-media-subdomains-20260602]
      :surface service-runtime-universe)
    (service :id xjp-mail-service
      :project xjp-mail-service
      :root "/Users/jinchen/Downloads/xiaojinpro-gateway/xiaojinpro-backend/services/mail"
      :intent ".missiond/intent.lisp"
      :backend ".missiond/backend/xjp-mail-service-blueprint.lisp"
      :environment production
      :api-base-url "https://mail.xiaojins.com/v1/mail"
      :domains ["mail.xiaojins.com"]
      :dns-provider cloudflare
      :dns-record (:type A :name "mail.xiaojins.com" :content "34.104.147.118" :proxied false :ttl 60 :authority xjp-domain-service)
      :deployment (:substrate deploy-center :dc_slug "xjp-mail-service" :runtime-target gcp-runtime :executor gcp-agent :container "xjp-mail-service" :default-port 8098 :authority release-provenance)
      :build-lane (:id privatecloud-rust-build-lane :builder privatecloud-10900kf :executor privatecloud-agent :source-sync deploy-center-codebase :dockerfile "services/mail/Dockerfile" :image "ghcr.io/xiaojinpro-team/xjp-mail-service" :artifact-lane cloud-registry-lane :authority deploy-center :target-side-build-prohibited true)
      :proxy (:kind caddy :domain "mail.xiaojins.com" :routes ["/health" "/health/*" "/v1/mail" "/v1/mail/*"] :upstream "localhost:8098")
      :ports (:http 8098)
      :health ["/health/live" "/health/ready"]
      :mail-policy (:provider google-workspace :mailbox-model hybrid :default-agent-mode draft-only :dns-authority xjp-domain-service :audit-table mail_audits)
      :dependencies [xjp-auth xjp-domain-service xjp-pg-prod secret-store deploy-center google-workspace-admin-sdk gmail-api cloud-pubsub]
      :ops-capability deploy-ops
      :source-evidence [xjp-mail-service-google-workspace-plan-20260602]
      :surface service-runtime-universe)
    (service :id xjp-legal-service
      :project xjp-legal-service
      :root "/Users/jinchen/Downloads/xiaojinpro-gateway/xiaojinpro-backend/services/legal"
      :intent ".missiond/intent.lisp"
      :backend ".missiond/backend/xjp-legal-service-blueprint.lisp"
      :operations ".missiond/operations/xjp-legal-service-operations-blueprint.lisp"
      :environment production
      :api-base-url "https://legal.xiaojins.com/v1/legal"
      :domains ["legal.xiaojins.com"]
      :dns-provider cloudflare
      :dns-record (:type A :name "legal.xiaojins.com" :content "34.104.147.118" :proxied false :ttl 60 :authority xjp-domain-service)
      :deployment (:substrate deploy-center :dc_slug "xjp-legal-service" :runtime-target gcp-runtime :executor gcp-agent :container "xjp-legal-service" :default-port 8099 :host-port 8099 :authority release-provenance)
      :build-lane (:id privatecloud-rust-build-lane :builder privatecloud-10900kf :executor privatecloud-agent :source-sync deploy-center-codebase :dockerfile "services/legal/Dockerfile" :image "ghcr.io/xiaojinpro-team/xjp-legal-service" :artifact-lane cloud-registry-lane :manifest "services/legal/service.manifest.toml" :authority deploy-center :target-side-build-prohibited true)
      :proxy (:kind caddy :domain "legal.xiaojins.com" :routes ["/health" "/health/*" "/v1/legal" "/v1/legal/*" "/v1/admin/legal" "/v1/admin/legal/*"] :upstream "localhost:8099")
      :ports (:http 8099 :host 8099)
      :health ["/health/live" "/health/ready"]
      :legal-policy (:partition xjp-global :public-read anonymous-published-policy :admin-auth xjp-admin-jwt :ledger-write xjp-auth-or-internal-token :audit-table legal_acceptance_events :consent-table legal_consent_events :support-mail "legal@xiaojins.com")
      :dependencies [xjp-auth xjp-pg-prod secret-store xjp-domain-service xjp-mail-service deploy-center]
      :ops-capability deploy-ops
      :source-evidence [services/legal/service.manifest.toml services/legal/deploy/deploy-center/project.json services/legal/.missiond/intent.lisp]
      :risks [live-domain-readiness-pending support-mailbox-readiness-pending deploy-center-release-closure-pending]
      :surface service-runtime-universe)
    (service :id xjp-invoice-service
      :project xjp-invoice-service
      :root "/Users/jinchen/Downloads/xiaojinpro-gateway/xiaojinpro-backend/services/invoice"
      :intent ".missiond/intent.lisp"
      :backend ".missiond/backend/xjp-invoice-service-blueprint.lisp"
      :operations ".missiond/operations/xjp-invoice-service-operations-blueprint.lisp"
      :environment production
      :api-base-url "https://invoice.xiaojins.com/v1/invoices"
      :domains ["invoice.xiaojins.com"]
      :dns-provider cloudflare
      :dns-record (:type A :name "invoice.xiaojins.com" :content "34.104.147.118" :proxied false :ttl 60 :authority xjp-domain-service)
      :deployment (:substrate deploy-center :dc_slug "xjp-invoice-service" :runtime-target gcp-runtime :executor gcp-agent :container "xjp-invoice-service" :default-port 8100 :host-port 8100 :container-port 8100 :authority release-provenance)
      :build-lane (:id privatecloud-rust-build-lane :builder privatecloud-10900kf :executor privatecloud-agent :source-sync deploy-center-codebase :dockerfile "services/invoice/Dockerfile" :image "ghcr.io/xiaojinpro-team/xjp-invoice-service" :artifact-lane cloud-registry-lane :manifest "services/invoice/service.manifest.toml" :authority deploy-center :target-side-build-prohibited true)
      :proxy (:kind caddy :domain "invoice.xiaojins.com" :routes ["/health" "/health/*" "/v1/invoices" "/v1/invoices/*"] :upstream "localhost:8100")
      :ports (:http 8100 :host 8100 :container-http 8100)
      :health ["/health/live" "/health/ready"]
      :invoice-policy (:partition xjp-global :mode manual-issue-v1 :currency CNY :providers [wechat alipay] :payments-lock-table invoice_order_locks :audit-table invoice_audit_events :red-letter-table invoice_red_letters)
      :dependencies [xjp-auth payments xjp-pg-prod secret-store xjp-domain-service xjp-mail-service deploy-center]
      :ops-capability deploy-ops
      :source-evidence [services/invoice/service.manifest.toml services/invoice/deploy/deploy-center/project.json services/invoice/.missiond/intent.lisp services/payments/migrations/20250101000015_create_invoice_order_locks.sql]
      :risks [live-domain-readiness-pending production-secret-binding-pending deploy-center-release-closure-pending]
      :surface service-runtime-universe)
    (service :id wepub
      :project wechat-publisher
      :root "/Users/jinchen/Projects/wechat-publisher"
      :intent ".missiond/intent.lisp"
      :backend ".missiond/backend/wechat-publisher-backend-blueprint.lisp"
      :environment production
      :public-base-url "https://www.wepub.top"
      :api-base-url "https://wepub-api.xiaojins.com"
      :domains ["wepub.top" "www.wepub.top" "wepub-api.xiaojins.com"]
      :compat-domains ["api.wepub.top"]
      :dns-provider xjp-domain-service
      :dns-record (:type A :name "wepub-api.xiaojins.com" :content "34.104.147.118" :proxied false :ttl 60 :authority xjp-domain-service)
      :deployment (:substrate deploy-center :dc_slug "wepub" :runtime-target gcp-runtime :executor gcp-agent :container "wepub" :default-port 8094 :authority release-provenance)
      :build-lane (:id privatecloud-rust-build-lane :builder privatecloud-10900kf :executor privatecloud-agent :source-sync deploy-center-codebase :dockerfile "backend/Dockerfile" :image "ghcr.io/xiaojinpro-team/wepub" :artifact-lane cloud-registry-lane :authority deploy-center :target-side-build-prohibited true)
      :proxy (:kind caddy :domain "wepub-api.xiaojins.com" :compat-domain "api.wepub.top" :upstream "localhost:8094")
      :ports (:http 8094)
      :health ["/api/health"]
      :dependencies [xjp-auth xjp-pg-prod stripe openrouter cloudflare]
      :ops-capability deploy-ops
      :source-evidence [skill:wepub wechat-publisher-project-ssot]
      :risks [subscription-webhook-secret-required bot-jwt-local-decode-audit]
      :surface service-runtime-universe)
    (service :id jinstudio
      :project jinstudio
      :root "/Users/jinchen/Downloads/xiaojinpro-gateway/xiaojinpro-backend/jinstudio-frontend"
      :intent ".missiond/intent.lisp"
      :frontend ".missiond/frontend/jinstudio-frontend-blueprint.lisp"
      :environment production
      :public-base-url "https://jinstudio.com"
      :domains ["jinstudio.com" "www.jinstudio.com"]
      :dns-provider cloudflare
      :deployment (:substrate lovable-or-static-host :framework vite-react :authority release-provenance)
      :health ["/"]
      :dependencies [supabase-lead-capture cloudflare]
      :ops-capability deploy-ops
      :source-evidence [jinstudio-project-ssot]
      :risks [supabase-lead-capture-policy-pending production-hosting-authority-needs-proof]
      :surface service-runtime-universe)
    (service :id secret-store
      :project secret-store
      :root "/Users/jinchen/Downloads/xiaojinpro-gateway/services/secret-store-rs"
      :intent ".missiond/intent.lisp"
      :environment production
      :public-base-url "https://ss.xiaojins.com"
      :domains ["ss.xiaojins.com"]
      :compat-domains ["ss.xiaojinpro.top"]
      :dns-provider xjp-domain-service
      :dns-record (:type A :name "ss.xiaojins.com" :content "34.104.147.118" :proxied false :ttl 60 :authority xjp-domain-service)
      :deployment (:substrate gcp-vm :runtime-target gcp-runtime :container "secret-store" :local-bind "127.0.0.1:8091" :proxy caddy :authority deploy-center-provenance)
      :build-lane (:id privatecloud-rust-build-lane :builder privatecloud-10900kf :executor privatecloud-agent :source-sync deploy-center-codebase :dockerfile "docker/Dockerfile" :image "ghcr.io/rickyjim626/secret-store-rs" :artifact-lane cloud-registry-lane :authority deploy-center :target-side-build-prohibited true)
      :proxy (:kind caddy :domain "ss.xiaojins.com" :compat-domain "ss.xiaojinpro.top" :upstream "localhost:8091")
      :health ["/health/live" "/health/ready" "/livez" "/readyz"]
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
      :build-lane (:id privatecloud-rust-build-lane :builder privatecloud-10900kf :executor privatecloud-agent :source-sync deploy-center-codebase :dockerfile "docker/Dockerfile" :image "ghcr.io/rickyjim626/secret-store-rs" :artifact-lane cn-oss-bundle-lane :authority deploy-center :target-side-build-prohibited true)
      :health ["/health/live" "/health/ready" "/livez" "/readyz"]
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
      :environment production
      :api-base-url "https://eventhub.xiaojins.com/v1/eventhub"
      :domains ["eventhub.xiaojins.com"]
      :dns-provider xjp-domain-service
      :dns-record (:type A :name "eventhub.xiaojins.com" :content "34.104.147.118" :proxied false :ttl 60 :authority xjp-domain-service)
      :deployment (:substrate deploy-center :dc_slug "xjp-eventhub" :runtime-target gcp-runtime :executor gcp-agent :container "xjp-eventhub" :default-port 8092 :host-bind "127.0.0.1:8092" :authority release-provenance)
      :local-runtime (:substrate launchd :label "com.xjp.eventhub.provider" :url "http://127.0.0.1:8092" :database "xjp_eventhub" :storage postgres-durable :bringup "scripts/manage-local-providers.sh")
      :proxy (:kind caddy :domain "eventhub.xiaojins.com" :routes ["/health" "/health/*" "/v1/eventhub" "/v1/eventhub/*"] :compat-domain "auth.xiaojinpro.com" :compat-routes ["/v1/eventhub/*"] :upstream "localhost:8092")
      :health ["/health" "/health/live" "/health/ready" "/v1/eventhub/status"]
      :dependencies [deploy-center timeline? postgres?]
      :ops-capability eventhub
      :surface service-runtime-universe)
    (service :id object-storage
      :project object-storage
      :root "/Users/jinchen/Downloads/xiaojinpro-gateway/xiaojinpro-backend/services/object-storage"
      :intent ".missiond/intent.lisp"
      :backend ".missiond/backend/object-storage-backend-blueprint.lisp"
      :environment production
      :api-base-url "https://files.xiaojins.com/v1/storage"
      :domains ["files.xiaojins.com"]
      :dns-provider xjp-domain-service
      :dns-record (:type A :name "files.xiaojins.com" :content "34.104.147.118" :proxied false :ttl 60 :authority xjp-domain-service)
      :deployment (:substrate deploy-center :dc_slug "xjp-object-storage" :runtime-target gcp-runtime :executor gcp-agent :container "xjp-object-storage" :default-port 8087 :host-bind "127.0.0.1:8087" :authority release-provenance)
      :build-lane (:id privatecloud-rust-build-lane :builder privatecloud-10900kf :executor privatecloud-agent :source-sync deploy-center-codebase :dockerfile "services/object-storage/Dockerfile" :image "ghcr.io/xiaojinpro-team/xjp-object-storage" :artifact-lane cloud-registry-lane :authority deploy-center :target-side-build-prohibited true)
      :proxy (:kind caddy :domain "files.xiaojins.com" :routes ["/health" "/health/*" "/v1/storage" "/v1/storage/*" "/v1/cloud-drive" "/v1/cloud-drive/*"] :compat-domain "auth.xiaojinpro.com" :compat-routes ["/api/storage/*"] :upstream "localhost:8087")
      :ports (:host 8087 :container 8087)
      :health ["/health" "/health/live" "/health/ready" "/v1/storage/admin/status"]
      :dependencies [xjp-auth xjp-pg-prod secret-store cloudflare-r2 aliyun-oss caddy xjp-domain-service]
      :ops-capability deploy-ops
      :source-evidence [services.yaml services/object-storage/Dockerfile xjp-domain-service-required-domain-files]
      :surface service-runtime-universe)
    (service :id xjp-project-universe
      :project xjp-project-universe
      :root "/Users/jinchen/Downloads/xiaojinpro-gateway/xiaojinpro-backend/services/project-universe"
      :intent ".missiond/intent.lisp"
      :backend ".missiond/backend/xjp-project-universe-backend-blueprint.lisp"
      :environment production
      :api-base-url "https://projects.xiaojins.com/v1/project-universe"
      :domains ["projects.xiaojins.com"]
      :dns-provider cloudflare
      :dns-record (:type A :name "projects.xiaojins.com" :content "34.104.147.118" :proxied false :ttl 60 :authority xjp-domain-service)
      :deployment (:substrate deploy-center :dc_slug "xjp-project-universe" :runtime-target gcp-runtime :executor gcp-agent :container "xjp-project-universe" :default-port 8101 :host-port 8102 :container-port 8101 :authority release-provenance)
      :build-lane (:id privatecloud-rust-build-lane :builder privatecloud-10900kf :executor privatecloud-agent :source-sync deploy-center-codebase :dockerfile "services/project-universe/Dockerfile" :image "ghcr.io/xiaojinpro-team/xjp-project-universe" :artifact-lane cloud-registry-lane :authority deploy-center :target-side-build-prohibited true)
      :proxy (:kind caddy :domain "projects.xiaojins.com" :routes ["/health" "/health/*" "/v1/project-universe" "/v1/project-universe/*"] :upstream "localhost:8102")
      :ports (:http 8101 :host 8102 :container-http 8101)
      :health ["/health/live" "/health/ready" "/v1/project-universe/status"]
      :read-model-policy (:cache in-memory-snapshot :authority-chain [missiond deploy-center xjp-domain-service forge] :raw-lisp-parsing prohibited :partial-source-diagnostics required)
      :dependencies [missiond deploy-center xjp-domain-service forge? secret-store?]
      :ops-capability project-management-read-model
      :source-evidence [services/project-universe/.missiond/intent.lisp services/project-universe/deploy/deploy-center/project.json]
      :surface service-runtime-universe)
    (service :id code-center
      :project code-center
      :root "/Users/jinchen/Downloads/xiaojinpro-gateway/xiaojinpro-backend/services/code-center"
      :intent ".missiond/intent.lisp"
      :backend ".missiond/backend/code-center-backend-blueprint.lisp"
      :environment production
      :api-base-url "https://code.xiaojins.com/api/code"
      :domains ["code.xiaojins.com"]
      :dns-provider xjp-domain-service
      :dns-record (:type A :name "code.xiaojins.com" :content "34.104.147.118" :proxied false :ttl 60 :authority xjp-domain-service)
      :deployment (:substrate deploy-center :dc_slug "xjp-code-center" :runtime-target gcp-runtime :executor gcp-agent :container "xjp-code-center" :default-port 8093 :host-bind "127.0.0.1:8093" :authority release-provenance)
      :build-lane (:id privatecloud-rust-build-lane :builder privatecloud-10900kf :executor privatecloud-agent :source-sync deploy-center-codebase :dockerfile "docker/Dockerfile.code-center" :image "ghcr.io/ruoqijin/xjp-code-center" :artifact-lane cloud-registry-lane :manifest "services/code-center/service.manifest.toml" :authority deploy-center :target-side-build-prohibited true)
      :proxy (:kind caddy :domain "code.xiaojins.com" :routes ["/health" "/health/*" "/api/code" "/api/code/*"] :upstream "localhost:8093")
      :ports (:http 8093)
      :health ["/health/live" "/health/ready" "/api/code/health"]
      :dependencies [xjp-auth xjp-pg-prod redis? secret-store?]
      :ops-capability deploy-ops
      :source-evidence [services/code-center/service.manifest.toml docker/Dockerfile.code-center deploy/gcp-vm/xjp-postgres-stack/docker-compose.yml]
      :surface service-runtime-universe)
    (service :id skill-store-gateway
      :project skill-store
      :root "/Users/jinchen/Downloads/xiaojinpro-gateway/xiaojinpro-backend/services/skill-store-gateway"
      :intent ".missiond/intent.lisp"
      :backend ".missiond/backend/skill-store-gateway-backend-blueprint.lisp"
      :environment production
      :api-base-url "https://skills.xiaojins.com"
      :domains ["skills.xiaojins.com"]
      :dns-provider xjp-domain-service
      :dns-record (:type A :name "skills.xiaojins.com" :content "34.104.147.118" :proxied false :ttl 60 :authority xjp-domain-service)
      :deployment (:substrate deploy-center :dc_slug "skill-store-gateway" :runtime-target gcp-runtime :executor gcp-agent :container "skill-store-gateway" :default-port 8901 :host-bind "127.0.0.1:8901" :authority release-provenance)
      :build-lane (:id privatecloud-rust-build-lane :builder privatecloud-10900kf :executor privatecloud-agent :source-sync deploy-center-codebase :dockerfile "services/skill-store-gateway/Dockerfile" :image "ghcr.io/xiaojinpro-team/skill-store-gateway" :artifact-lane cloud-registry-lane :manifest "services/skill-store-gateway/service.manifest.toml" :authority deploy-center :target-side-build-prohibited true)
      :proxy (:kind caddy :domain "skills.xiaojins.com" :routes ["/health" "/health/*" "/*"] :upstream "localhost:8901")
      :ports (:http 8901 :legacy-upstream 8900)
      :health ["/health/live" "/health/ready"]
      :dependencies [skill-store-legacy deploy-center caddy]
      :ops-capability deploy-ops
      :source-evidence [services/skill-store-gateway/service.manifest.toml deploy/gcp-vm/xjp-postgres-stack/docker-compose.yml skill-store-legacy-tcp-health-20260604]
      :risks [legacy-skill-store-http-health-missing]
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
    (service :id good-things-daily
      :project good-things-daily
      :root "/Users/jinchen/Projects/good-things-daily"
      :intent ".missiond/intent.lisp"
      :backend ".missiond/backend/good-things-daily-backend-blueprint.lisp"
      :frontend ".missiond/frontend/good-things-daily-frontend-blueprint.lisp"
      :operations ".missiond/operations/good-things-daily-operations-blueprint.lisp"
      :environment production
      :public-base-url "https://goodnews.xiaojinpro.top"
      :frontend-url "https://goodnews.xiaojinpro.top"
      :api-base-url "https://goodnews-api.xiaojins.com/api"
      :domains ["goodnews.xiaojinpro.top" "goodnews-api.xiaojins.com"]
      :compat-domains ["goodnews-api.xiaojinpro.top"]
      :dns-provider xjp-domain-service
      :dns-records [(:type CNAME :name "goodnews.xiaojinpro.top" :content "cname.vercel-dns.com" :proxied false :authority xjp-domain-service :status planned)
                    (:type A :name "goodnews-api.xiaojins.com" :content "34.104.147.118" :proxied false :ttl 60 :authority xjp-domain-service)]
      :build-lane (:id privatecloud-rust-build-lane :builder privatecloud-10900kf :executor privatecloud-agent :source-sync deploy-center-codebase :dockerfile "backend/Dockerfile" :image "ghcr.io/ruoqijin/good-things-daily-backend" :artifact-lane cloud-registry-lane :manifest "service.manifest.toml" :target-side-build-prohibited true :provenance [source_commit builder_id image_digest target_image rollback_image])
      :deployment (:substrate gcp-vm :runtime-target gcp-runtime :container "good-things-daily-backend" :local-bind "127.0.0.1:4017" :proxy caddy :database "xjp-pg-prod/good_things_daily" :compose "deploy/gcp-vm/compose.yaml" :image-env GOOD_THINGS_BACKEND_IMAGE :target-side-build-prohibited true :authority deploy-center-provenance)
      :frontend-deployment (:substrate vercel :project "rickyjim626/good-things-daily" :root-directory "frontend" :production-domain "goodnews.xiaojinpro.top")
      :health ["https://goodnews.xiaojinpro.top/" "https://goodnews-api.xiaojins.com/api/health" "https://goodnews-api.xiaojins.com/api/v1/feed/today?lang=zh" "https://goodnews-api.xiaojins.com/api/v1/digests/today?lang=zh"]
      :dependencies [xjp-router xjp-pg-prod secret-store vercel cloudflare xjp-domain-service]
      :llm-provider (:authority xjp-router :endpoint "/v1/chat/completions" :model "claude-opus-4-6" :env [XJP_ROUTER_BASE_URL XJP_ROUTER_SERVICE_TOKEN GOOD_THINGS_TITLE_MODEL] :rule "Home feed headlines are story_presentations.what_happened values created through xjp-router claude-opus-4-6 prompt open-door-joy-presentation-v1; generated_title is the fallback/detail headline; source_title remains evidence only and must not be rendered as the primary public title.")
      :ops-capability deploy-ops
      :source-evidence ["/Users/jinchen/Projects/good-things-daily/.missiond/intent.lisp" "/Users/jinchen/Projects/good-things-daily/.missiond/check.sh" "/Users/jinchen/Projects/good-things-daily/service.manifest.toml" "/Users/jinchen/Projects/good-things-daily/deploy/deploy-center/project.json"]
      :risks [presentation-layer-regression-pending feedback-event-regression-pending feed-browser-smoke-pending scheduled-job-runner-pending]
      :surface service-runtime-universe)
    (capability :id cloudflare-dns
      :provider cloudflare
      :default-mode read-only-inventory
      :mutating-policy "Cloudflare DNS mutation requires xjp-domain-service, Secret Store / Deploy Center secret binding, deploy-ops capability, and explicit Board approval; workers must report unavailable rather than pretend they can operate DNS when credentials or approval are absent."
      :secrets [CLOUDFLARE_API_TOKEN_REF CLOUDFLARE_API_TOKEN CLOUDFLARE_ACCOUNT_ID CLOUDFLARE_ZONE_ID DOMAIN_APPROVAL_TOKEN_REF DOMAIN_APPROVAL_TOKEN]
      :surface service-runtime-universe)
    (capability :id aliyun-dns
      :provider aliyun
      :default-mode read-only-inventory
      :mutating-policy "Aliyun DNS mutation requires xjp-domain-service, Secret Store / Deploy Center secret binding, deploy-ops capability, and explicit Board approval; workers must report unavailable rather than operate Aliyun DNS directly when credentials or approval are absent."
      :secrets [ALIYUN_ACCESS_KEY_ID_REF ALIYUN_ACCESS_KEY_SECRET_REF ALIYUN_ACCESS_KEY_ID ALIYUN_ACCESS_KEY_SECRET DOMAIN_APPROVAL_TOKEN_REF DOMAIN_APPROVAL_TOKEN]
      :surface service-runtime-universe))
