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
      :deployment (:substrate deploy-center :dc_slug "xjp-deploy-center" :runtime-target gcp-runtime :executor gcp-agent :container "xjp-deploy-center" :default-port 8090 :authority release-provenance :provenance-api "/api/deploy/provenance/:project")
      :deployment-confirmation (:checker "node scripts/check-m6-deployment-status.mjs --json" :status-api "/api/deploy/status" :rollout-workflow ".missiond/workflows/m6-deployment-rollout.lisp")
      :event-ingest (:endpoint "/webhooks/deploy-center-event" :domain system :event ExternalServiceEvent :source deploy_events :token-env MISSIOND_EXTERNAL_WEBHOOK_TOKEN :authority deploy-center.deploy_events :rule "deploy-center relays durable deploy_events rows into MissionD EventBridge with stable event_id and MissionD idempotency; MissionD must not infer production release state by stitching GitHub/curl/git when deploy-center has provenance.")
      :events [deploy_created build_started build_succeeded build_failed deploy_started deploy_succeeded deploy_failed workflow_run_created workflow_job_started workflow_job_succeeded workflow_job_failed workflow_job_cancelled workflow_job_lease_expired artifact_recorded smoke_succeeded smoke_failed rollback_started rollback_succeeded rollback_failed agent_heartbeat agent_update_started agent_update_succeeded agent_update_failed provenance_changed closure_verdict]
      :health ["/api/deploy/health" "/api/deploy/healthz/db"]
      :dependencies [xjp-pg-prod secret-store deploy-agent github-actions ghcr]
      :ops-capability deploy-ops
      :source-evidence [services/deploy-center/service.manifest.toml services.yaml deploy/gcp-vm/xjp-postgres-stack/docker-compose.yml]
      :surface service-runtime-universe)
    (service :id missiond-jarvis-edge
      :project missiond
      :root "/Users/jinchen/Projects/missiond"
      :environment production
      :public-base-url "https://jarvis.xiaojinpro.top"
      :domains ["jarvis.xiaojinpro.top"]
      :dns-provider cloudflare
      :dns-record (:type A :name "jarvis.xiaojinpro.top" :content "34.104.147.118" :proxied false :ttl 60 :authority cloudflare)
      :deployment (:substrate gcp-caddy-edge :runtime-target gcp-runtime :origin "104.194.81.38:9876" :tunnel-client "rickyhqmac-mini" :target-service missiond :authority verified-smoke)
      :proxy (:kind caddy :domain "jarvis.xiaojinpro.top" :routes ["/health" "/v1/*" "/api/monitor/jarvis" "/api/readiness" "/jarvis/*"] :upstream "104.194.81.38:9876" :sse-no-buffer true)
      :ports (:https 443)
      :health ["/health" "/api/readiness" "/api/monitor/jarvis" "/jarvis/api/monitor/jarvis"]
      :dependencies [gcp-runtime caddy cloudflare-dns bwg-tunnel rickyhq-macmini-m4 missiond-daemon]
      :ops-capability deploy-ops
      :source-evidence [jarvis-xiaojinpro-top-cloudflare-dns-20260528 gcp-caddy-jarvis-edge-20260528 missiond-jarvis-sse-smoke-20260528]
      :risks [dns-local-cache-propagation]
      :surface service-runtime-universe)
    (service :id search-center
      :project search-center
      :root "/Users/jinchen/Downloads/xiaojinpro-gateway/xiaojinpro-backend/services/search-center"
      :intent ".missiond/intent.lisp"
      :backend ".missiond/backend/search-center-backend-blueprint.lisp"
      :environment production
      :public-base-url "https://search-center.xiaojinpro.top"
      :frontend-url "https://search.xiaojinpro.top"
      :domains ["search.xiaojinpro.top" "search-center.xiaojinpro.top" "auth.xiaojinpro.com"]
      :dns-provider cloudflare
      :dns-records [(:type CNAME :name "search.xiaojinpro.top" :content "cname.vercel-dns.com" :proxied false :ttl 60 :authority cloudflare)
                    (:type A :name "search-center.xiaojinpro.top" :content "34.104.147.118" :proxied false :ttl 60 :authority cloudflare)]
      :deployment (:substrate deploy-center :dc_slug "xjp-search-center" :runtime-target gcp-runtime :executor gcp-agent :container "xjp-search-center" :default-port 3120 :authority release-provenance)
      :proxy (:kind caddy :domain "search-center.xiaojinpro.top" :routes ["/health/live" "/health/ready" "/v1/health" "/v1/me" "/v1/search" "/v1/search/*" "/v1/research" "/v1/research/*" "/v1/history" "/v1/history/*"])
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
      :api-base-url "https://auth.xiaojinpro.com/payments"
      :domains ["auth.xiaojinpro.com"]
      :deployment (:substrate deploy-center :dc_slug "xjp-payments" :runtime-target gcp-runtime :executor gcp-agent :container "xjp-payments" :default-port 8080 :host-bind "127.0.0.1:3109" :authority release-provenance)
      :proxy (:kind caddy :domain "auth.xiaojinpro.com" :routes ["/payments" "/payments/*"] :upstream "localhost:3109")
      :ports (:host 3109 :container 8080)
      :health ["/payments/health" "/payments/health/ready" "/payments/health/runtime"]
      :dependencies [xjp-auth xjp-router xjp-pg-prod secret-store stripe wechatpay alipay]
      :ops-capability deploy-ops
      :source-evidence [skill:services/payments services/payments/service.manifest.toml payments-provenance-20260602]
      :risks [stripe-live-secret-pending activation-code-test-channel-required multi-product-payment-config-readiness]
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
      :api-base-url "https://auth.xiaojinpro.com/asr"
      :domains ["speechscribe.top" "www.speechscribe.top" "asr.xiaojinpro.top" "xjp-asr-web.vercel.app" "auth.xiaojinpro.com"]
      :dns-provider cloudflare
      :dns-records [(:type A :name "speechscribe.top" :content "76.76.21.21" :proxied false :authority cloudflare)
                    (:type CNAME :name "www.speechscribe.top" :content "cname.vercel-dns.com" :proxied false :authority cloudflare)
                    (:type A :name "asr.xiaojinpro.top" :content "76.76.21.21" :proxied false :authority cloudflare)]
      :deployment (:substrate deploy-center :dc_slug "xjp-asr" :runtime-target gcp-runtime :executor gcp-agent :container "xjp-asr" :default-port 8090 :host-bind "127.0.0.1:8089" :authority release-provenance)
      :frontend-deployment (:substrate vercel :project "rickyjim626s-projects/xjp-asr-web" :production-domain "speechscribe.top" :fallback-domain "xjp-asr-web.vercel.app")
      :proxy (:kind caddy :domain "auth.xiaojinpro.com" :routes ["/asr" "/asr/*"] :upstream "localhost:8089")
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
      :proxy (:kind caddy :domain "images.xiaojins.com" :routes ["/health" "/health/*" "/v1/images" "/v1/images/*"])
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
      :runner (:kind deploy-agent-hosted-service :project xjp-video-transcode-runner :binary "xjp-video-transcode-runner" :runtime-target windows-12900kf :agent_url windows :transport self-built-proxy-deploy-program :queue video_jobs :ffmpeg cpu-required :profiles [poster_jpeg mp4_passthrough hls_720p])
      :proxy (:kind caddy :domain "videos.xiaojins.com" :routes ["/health" "/health/*" "/v1/videos" "/v1/videos/*"])
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
      :proxy (:kind caddy :domain "domains.xiaojins.com" :routes ["/health" "/health/*" "/v1/domains" "/v1/domains/*"])
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
      :proxy (:kind caddy :domain "mail.xiaojins.com" :routes ["/health" "/health/*" "/v1/mail" "/v1/mail/*"])
      :ports (:http 8098)
      :health ["/health/live" "/health/ready"]
      :mail-policy (:provider google-workspace :mailbox-model hybrid :default-agent-mode draft-only :dns-authority xjp-domain-service :audit-table mail_audits)
      :dependencies [xjp-auth xjp-domain-service xjp-pg-prod secret-store deploy-center google-workspace-admin-sdk gmail-api cloud-pubsub]
      :ops-capability deploy-ops
      :source-evidence [xjp-mail-service-google-workspace-plan-20260602]
      :surface service-runtime-universe)
    (service :id wepub
      :project wechat-publisher
      :root "/Users/jinchen/Projects/wechat-publisher"
      :intent ".missiond/intent.lisp"
      :backend ".missiond/backend/wechat-publisher-backend-blueprint.lisp"
      :environment production
      :public-base-url "https://www.wepub.top"
      :api-base-url "https://api.wepub.top"
      :domains ["wepub.top" "www.wepub.top" "api.wepub.top"]
      :dns-provider cloudflare
      :deployment (:substrate deploy-center :dc_slug "wepub" :runtime-target gcp-runtime :executor gcp-agent :container "wepub" :default-port 8094 :authority release-provenance)
      :proxy (:kind caddy :domain "api.wepub.top" :upstream "localhost:8094")
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
      :api-base-url "https://goodnews-api.xiaojinpro.top/api"
      :domains ["goodnews.xiaojinpro.top" "goodnews-api.xiaojinpro.top"]
      :dns-provider cloudflare
      :dns-records [(:type CNAME :name "goodnews.xiaojinpro.top" :content "cname.vercel-dns.com" :proxied false :authority xjp-domain-service :status planned)
                    (:type A :name "goodnews-api.xiaojinpro.top" :content "34.104.147.118" :proxied false :authority xjp-domain-service :status planned)]
      :build-lane (:id privatecloud-rust-build-lane :builder privatecloud-10900kf :executor privatecloud-agent :source-sync deploy-center-codebase :dockerfile "backend/Dockerfile" :image "ghcr.io/ruoqijin/good-things-daily-backend" :artifact-lane cloud-registry-lane :manifest "service.manifest.toml" :target-side-build-prohibited true :provenance [source_commit builder_id image_digest target_image rollback_image])
      :deployment (:substrate gcp-vm :runtime-target gcp-runtime :container "good-things-daily-backend" :local-bind "127.0.0.1:4017" :proxy caddy :database "xjp-pg-prod/good_things_daily" :compose "deploy/gcp-vm/compose.yaml" :image-env GOOD_THINGS_BACKEND_IMAGE :target-side-build-prohibited true :authority deploy-center-provenance)
      :frontend-deployment (:substrate vercel :project "rickyjim626/good-things-daily" :root-directory "frontend" :production-domain "goodnews.xiaojinpro.top")
      :health ["https://goodnews.xiaojinpro.top/" "https://goodnews-api.xiaojinpro.top/api/health" "https://goodnews-api.xiaojinpro.top/api/v1/feed/today?lang=zh" "https://goodnews-api.xiaojinpro.top/api/v1/digests/today?lang=zh"]
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
      :surface service-runtime-universe))
