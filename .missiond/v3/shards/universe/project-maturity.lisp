(project-maturity-model
    :schema "missiond.project-maturity-model.v2"
    :rule "M6 is the highest maturity level and means Auth-grade production-ready SSOT/code/runtime/test clarity: domain model, policy, flow, event, runtime projection, implementation map, compatibility ledger, hot-path wiring, regression matrix, source hygiene, and data-residency declarations for data-bearing projects are fine-grained, code-aligned, and formatter-converged."
    :gate "scripts/check-project-maturity.mjs --min-level M5 is the default universe operational gate for active/runtime-owned projects; incubating projects remain visible with gaps but do not block the global M5 gate until promoted. scripts/check-project-maturity.mjs --min-level M6 proves Auth-grade final maturity."
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
       "Incubating projects MAY be registered below M5 with explicit gaps; they are shown in Universe and can run their own checkers, but they are excluded from the global M5 operational gate until their registry status is promoted out of incubating-project."
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
    (maturity :id search-center :current M5 :target M6 :gap [external-provider-secret-import deep-research-live-300-source-artifact cross-domain-benchmark-suite production-browser-oauth-flow final-m6-report final-promotion-artifact-verification])
    (maturity :id payments :current M6 :target M6 :gap [])
    (maturity :id asr :current M6 :target M6 :gap [])
    (maturity :id timeline :current M5 :target M6 :gap [revision-event-authority service-event-regressions final-m6-report])
    (maturity :id pcea :current M6 :target M6 :gap [])
    (maturity :id xiaojinpro-ios :current M6 :target M6 :gap [])
    (maturity :id xiaojinpro-frontend :current M2 :target M6 :gap [frontend-domain-model auth-flow-regressions production-deploy-provenance final-m6-report])
    (maturity :id secret-store :current M5 :target M6 :gap [secret-version-rotation-domain capability-regressions final-m6-report])
    (maturity :id xiaojin-blog :current M5 :target M6 :gap [content-publishing-domain deploy-auth-boundary final-m6-report])
    (maturity :id jinstudio :current M2 :target M6 :gap [lead-capture-data-contract supabase-lead-regression production-deploy-provenance final-m6-report])
    (maturity :id cuthub :current M5 :target M6 :gap [community-domain auth-product-dependency final-m6-report])
    (maturity :id legacy-refactor-service :current M5 :target M6 :gap [deep-code-rewrite-worker customer-frontend forge-runtime-provider production-deploy-provenance final-m6-report])
    (maturity :id palm-era :current M6 :target M6 :gap [])
    (maturity :id chat-translator :current M6 :target M6 :gap [])
    (maturity :id long-image-service :current M6 :target M6 :gap [])
    (maturity :id wechat-publisher :current M6 :target M6 :gap [])
    (maturity :id problem-tutor :current M5 :target M6 :gap [postgres-durable-persistence router-model-alias-verification final-m6-report])
    (maturity :id daily-spark :current M6 :target M6 :gap []))
