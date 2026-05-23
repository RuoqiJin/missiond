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
