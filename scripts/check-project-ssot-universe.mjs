#!/usr/bin/env node

import fs from 'node:fs';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import { readBlueprintWithEvidenceSidecars } from './lib/v3_blueprint_contract_source.mjs';
import { maybeRunLispc, runLispc } from './lib/ocaml_lispc.mjs';

const usage = `Usage:
  node scripts/check-project-ssot-universe.mjs [--json] [--engine=auto|js|ocaml]

Checks MissionD multi-project SSOT registry convergence:
  - OCaml typed universe projection names MissionD, Board, Forge, Part1 devtools,
    XJP services, PCEA, plus App + external-infra projects and independent
    app projects xiaojinpro-frontend, search-center, palm-era,
    chat-translator, long-image-service, wechat-publisher, problem-tutor,
    jinstudio, daily-spark, good-things-daily, and diandian.
  - V3 service-runtime-universe exposes production service deployment facts.
  - V3 service-layer-template standardizes new product-service-layer scaffolds.
  - project-ssot-convergence workflow exists.
  - XJP and PCEA local SSOT checkers pass; every Part1 devtools project executes
    its declared cheap/static project-local runner (jarvis and jarvis-forge run
    bash .missiond/check.sh in default mode; jarvis-mechanic and xjpcode run
    their node SSOT checkers; neural-codegen and semantic-terminal run
    check.sh --dry-run); secret-store / xiaojin-blog / cuthub each execute
    bash .missiond/check.sh in default mode (read-only, sub-second).
`;

const COMPILED_UNIVERSE = '.missiond/v3/runtime/compiled/compiled-project-universe.json';
const MANAGEMENT_DOMAINS = new Set([
  'missiond-production-system',
  'xiaojinpro-core-backend',
  'xiaojinpro-platform-ops',
  'xiaojinpro-client-surface',
  'product-service-layer',
  'brand-content-site',
  'external-infra',
]);
const RUNTIME_LAYERS = new Set([
  'control-plane-frontend',
  'devtool',
  'platform-monorepo',
  'support-backend',
  'support-agent',
  'ops-service',
  'ops-agent',
  'ops-tool',
  'platform-frontend',
  'mobile-client',
  'public-content-site',
  'brand-site',
  'product-fullstack',
  'product-api',
  'product-frontend',
  'external-infra-runtime',
]);
const PROJECT_CHECKERS = new Map([
  // JS owns execution policy for cheap/static local runners. OCaml owns project id/root/maturity facts.
  ['jarvis', ['bash', ['.missiond/check.sh']]],
  ['jarvis-forge', ['bash', ['.missiond/check.sh']]],
  ['jarvis-mechanic', ['node', ['scripts/check-mechanic-ssot.mjs']]],
  ['xjpcode', ['node', ['scripts/check-xjpcode-ssot-complete.mjs', '--json']]],
  ['neural-codegen', ['bash', ['.missiond/check.sh', '--dry-run']]],
  ['semantic-terminal', ['bash', ['.missiond/check.sh', '--dry-run']]],
  ['xjp-mcp', ['bash', ['.missiond/check.sh']]],
  ['xjp-cli', ['bash', ['.missiond/check.sh']]],
  ['xjp-memory', ['bash', ['.missiond/check.sh']]],
  ['xjp-eventhub', ['bash', ['.missiond/check.sh']]],
  ['xjp-domain-service', ['bash', ['.missiond/check.sh']]],
  ['xjp-mail-service', ['bash', ['.missiond/check.sh']]],
  ['deploy-agent', ['bash', ['.missiond/check.sh']]],
  ['asr', ['bash', ['.missiond/check.sh']]],
  ['search-center', ['bash', ['.missiond/check.sh']]],
  ['pcea', ['node', ['scripts/check-pcea-ssot-complete.mjs', '--json']]],
  ['xiaojinpro-frontend', ['bash', ['.missiond/check.sh']]],
  ['secret-store', ['bash', ['.missiond/check.sh']]],
  ['xiaojin-blog', ['bash', ['.missiond/check.sh']]],
  ['jinstudio', ['bash', ['.missiond/check.sh']]],
  ['cuthub', ['bash', ['.missiond/check.sh']]],
  ['legacy-refactor-service', ['node', ['scripts/check-legacy-refactor-ssot.mjs', '--json']]],
  ['palm-era', ['bash', ['.missiond/check.sh']]],
  ['chat-translator', ['bash', ['.missiond/check.sh']]],
  ['long-image-service', ['bash', ['.missiond/check.sh']]],
  ['wechat-publisher', ['bash', ['.missiond/check.sh']]],
  ['problem-tutor', ['bash', ['.missiond/check.sh']]],
  ['daily-spark', ['bash', ['.missiond/check.sh']]],
  ['good-things-daily', ['bash', ['.missiond/check.sh']]],
  ['diandian', ['bash', ['.missiond/check.sh']]],
]);

function main() {
  const opts = parseArgs(process.argv.slice(2));

  const diagnostics = [];
  const engine = runOcamlUniverseCheck(opts.engine);
  if (engine.strictResult) {
    diagnostics.push(...(engine.strictResult.diagnostics ?? []).map((d) => ({
      file: d.file ?? '.missiond/v3/missiond-blueprint.lisp',
      message: `${d.code ?? 'OCAML_UNIVERSE'}: ${d.message}`,
    })));
  } else if (engine.mode === 'ocaml' && engine.ok === false) {
    diagnostics.push(...(engine.diagnostics ?? []).map((d) => ({
      file: d.file ?? '.missiond/v3/missiond-blueprint.lisp',
      message: `${d.code ?? 'OCAML_UNIVERSE'}: ${d.message}`,
    })));
  }
  const typedUniverse = loadTypedUniverseProjects(opts.engine);
  diagnostics.push(...typedUniverse.diagnostics);
  const projects = typedUniverse.projects.map((project) => ({
    ...project,
    checker: PROJECT_CHECKERS.get(project.id) ?? null,
  }));

  const blueprint = readBlueprintWithEvidenceSidecars(process.cwd(), '.missiond/v3/missiond-blueprint.lisp');
  requireAll(diagnostics, '.missiond/v3/missiond-blueprint.lisp', blueprint, [
    '(project-maturity-model',
    ':schema "missiond.project-maturity-model.v2"',
    '(level M5 :name worker-operational',
    '(level M6 :name auth-grade',
    '(project-maturity-registry',
	    ':schema "missiond.project-maturity-registry.v2"',
	    ':default-target M6',
	    ':common-m5-to-m6-gap [domain-model policy-flow-event-split compatibility-ledger hot-path-wiring regression-matrix data-residency-declaration final-m6-report]',
	    '(project-blueprint-registry',
    '(project-management-taxonomy',
    ':schema "missiond.project-management-taxonomy.v1"',
    ':fields [management-domain runtime-layer deployment-channels deployment-channel-drift]',
    '(deployment-channel-plane',
    ':schema "missiond.deployment-channel-plane.v1"',
    '(service-layer-template',
    ':schema "missiond.service-layer-template.v1"',
    ':id product-service-layer-standard',
    ':operator-guide "docs/guides/product-service-layer-template.md"',
    ':applies-to [product-service-layer]',
    ':runtime-layers [product-fullstack product-api product-frontend]',
    '(example :project palm-era :aliases ["椰岛纪元"] :shape product-fullstack)',
    '(example :project search-center :aliases ["聚合搜索" "搜索中心"] :shape product-fullstack)',
    '(example :project wechat-publisher :aliases [wepub "公众号发布"] :shape product-fullstack)',
    '(example :project asr :aliases [ASR "语音识别"] :shape product-fullstack)',
    '(example :project long-image-service :aliases ["长图"] :shape product-fullstack)',
    '(example :project chat-translator :aliases ["chat翻译"] :shape product-fullstack)',
    '(support-mail-standard',
    '(decision :id repo-placement',
    ':root-pattern "/Users/jinchen/Projects/<project-id>"',
    ':root-pattern "/Users/jinchen/Downloads/xiaojinpro-gateway/xiaojinpro-backend/services/<service-id>"',
    '(decision :id frontend-stack',
    'Next.js 16 + React 19 + TypeScript + Tailwind 4',
    '(decision :id backend-stack',
    'Rust axum + sqlx + PostgreSQL',
    'deploy_project_stage_configs.build.config.deploy_type=native_workflow',
    'deployment-channel-plane',
    '(auth-standard',
    ':default-provider xjp-auth',
    ':public-client-flow pkce',
    ':allowlist-rule "Every production, www, compatibility, and Vercel/custom domain that can initiate login MUST be present in the OAuth client redirect allowlist before or with domain cutover; wildcards are forbidden."',
    'xjp_oauth_client_update',
    'Invalid redirect_uri',
    'oauth-redirect-allowlist',
    'marker-cookie-gate',
    'jwks-issuer',
    '(payment-standard',
    ':default-provider xjp-payments',
    'product-code',
    'entitlement-policy',
    '(database-standard',
    'Supabase session pooler port 5432',
    '(decision :id rust-build-lane',
    '(rust-build-lane-standard',
    ':builder privatecloud-10900kf',
    'Rust product-service backend builds MUST run through deploy-center approved privatecloud/codebase build lane',
    'production runtime targets such as gcp-runtime/GCP VM are deploy targets, not Rust builders',
    'docker compose up --build',
    '(secret-store-standard',
    ':secret-path-pattern "projects/<project-id>/<environment>/<SECRET_NAME>"',
    '(vercel-standard',
    ':default-target vercel-frontend',
    ':backend-entrypoint "backend/api/axum.rs"',
    '(project-local-ssot-scaffold',
    '.missiond/backend/<project-id>-backend-blueprint.lisp',
    '(missiond-registration-scaffold',
    ':management-domain product-service-layer',
    ':default-maturity (:current M2 :target M6',
    '(agent-bootstrap-flow',
    'classify-product-service',
    'configure-auth',
    'configure-support-mail',
    'configure-data-payment-secrets',
    'register-missiond-universe',
    '(service-runtime-universe',
    ':schema "missiond.service-runtime-universe.v1"',
    '(service :id auth',
    ':project xiaojinpro-backend',
    ':root "/Users/jinchen/Downloads/xiaojinpro-gateway/xiaojinpro-backend/services/auth"',
    ':public-base-url "https://auth.xiaojinpro.com"',
    ':issuer "https://auth.xiaojinpro.com"',
    ':domains ["auth.xiaojinpro.com"]',
    ':dns-provider cloudflare',
    ':mutate requires-board-approval',
    ':deployment (:substrate kubernetes :namespace production :deployment "xjp-auth-center" :service "xjp-auth-center"',
    ':proxy (:kind caddy :domain "auth.xiaojinpro.com"',
    ':health ["/health/live" "/health/ready" "/.well-known/openid-configuration" "/.well-known/jwks.json"]',
    ':event-ingest (:endpoint "/webhooks/auth-event" :domain system :event ExternalServiceEvent',
    ':source auth-audit-events',
    ':token-env MISSIOND_EXTERNAL_WEBHOOK_TOKEN',
    ':authority provider-durable-log-first',
    ':dependencies [postgres redis secret-store wechat-open-platform google-oauth sms-provider email-provider]',
    ':ops-capability deploy-ops',
    '(service :id missiond-jarvis-edge',
    ':project missiond',
    ':public-base-url "https://jarvis.xiaojinpro.top"',
    ':domains ["jarvis.xiaojinpro.top"]',
    ':dns-record (:type A :name "jarvis.xiaojinpro.top" :content "34.104.147.118" :proxied false :ttl 60 :authority cloudflare)',
    ':deployment (:substrate gcp-caddy-edge :runtime-target gcp-runtime :origin "104.194.81.38:9876"',
    ':tunnel-client "rickyhqmac-mini-jarvis"',
    ':proxy (:kind caddy :domain "jarvis.xiaojinpro.top"',
    ':routes ["/health" "/v1/*" "/api/monitor/jarvis" "/api/readiness" "/jarvis/*"]',
    ':sse-no-buffer true',
    ':flush-interval "-1"',
    ':read-timeout "75s"',
    ':write-timeout "75s"',
    ':stream-timeout "0"',
    ':jarvis-runtime-topology (:schema "missiond.jarvis-runtime-topology.v1"',
    ':tunnel-server-url "ws://104.194.81.38:9876/tunnel/ws"',
    ':expected-deploy-agent-version "10.7.15"',
    ':launchd-unit "com.xiaojinpro.jarvis-tunnel"',
    ':health ["/health" "/api/readiness" "/api/monitor/jarvis" "/jarvis/api/monitor/jarvis"]',
    '(service :id good-things-daily',
    ':build-lane (:id privatecloud-rust-build-lane',
    ':builder privatecloud-10900kf',
    ':image "ghcr.io/ruoqijin/good-things-daily-backend"',
    ':image-env GOOD_THINGS_BACKEND_IMAGE',
    ':target-side-build-prohibited true',
	    '(data-residency-universe',
	    ':schema "missiond.data-residency-universe.v1"',
	    '(data-region-partition-contract',
	    '(xjp-platform-partition-contract',
	    'xjp-cn',
	    'xjp-global',
	    'xjp-global-eu',
	    'pcea-cn :platform xjp-cn',
	    'pcea-global :platform xjp-global',
	    'cuthub-cn :platform xjp-cn',
	    'cuthub-global :platform xjp-global',
	    '(regional-auth-issuer-contract',
    '(regional-storage-contract',
    '(regional-payment-ledger-contract',
    '(regional-router-model-policy',
    '(cross-region-data-policy',
    '(project-region-declaration :project pcea',
    ':data-regions [cn global]',
    ':contains-spi true',
    ':contains-important-data unknown',
    ':cross-region-default deny',
    'cuthub.cn',
    'cuthub.com',
    '(capability :id cloudflare-dns',
    ':default-mode read-only-inventory',
    'explicit Board approval',
  ]);
  checkMaturityRegistry(diagnostics, typedUniverse.maturity);
  checkProjectMaturityCoverage(diagnostics, typedUniverse.projects, typedUniverse.maturity);
  checkProjectTaxonomy(diagnostics, typedUniverse.projects);

	  requireExistingText(diagnostics, '/Users/jinchen/Downloads/xiaojinpro-gateway/xiaojinpro-backend/services/auth/.missiond/intent.lisp', [
	    '(intent auth-center',
	    '(env ISSUER',
	    ':example "https://auth.xiaojinpro.com"',
	    '(component google',
	    '(component wechat',
	  ]);
	  requireExistingText(diagnostics, '/Users/jinchen/Downloads/xiaojinpro-gateway/xiaojinpro-backend/services/auth/.missiond/intent-flow-google-oauth.lisp', [
	    '(flow google-oauth-login',
	    ':path "/auth/google/callback"',
	    'tenant_id',
	  ]);
	  requireExistingText(diagnostics, '/Users/jinchen/Downloads/xiaojinpro-gateway/xiaojinpro-backend/services/auth/.missiond/intent-db-iam.lisp', [
	    '(table tenant_users',
	    ':unique (tenant_id user_id)',
	  ]);

  const workflowPath = '.missiond/workflows/project-ssot-convergence.lisp';
  if (!fs.existsSync(workflowPath)) {
    diagnostics.push({ file: workflowPath, message: 'missing project SSOT convergence workflow' });
  } else {
    const workflow = fs.readFileSync(workflowPath, 'utf8');
    requireAll(diagnostics, workflowPath, workflow, [
      '(workflow project-ssot-convergence',
      ':schema "missiond.workflow.project-ssot-convergence.v1"',
      'collect-evidence',
      'draft-l1-index',
      'draft-backend-blueprint',
      'draft-frontend-blueprint',
      'create-checkers',
      'dispatch-backfill-workers',
      'verify-and-report',
    ]);
  }

  requireExistingText(diagnostics, 'docs/guides/product-service-layer-template.md', [
    'Product Service Layer Template',
    'management-domain=product-service-layer',
    'Next.js 16 + React 19 + TypeScript + Tailwind 4',
    'Rust axum + sqlx + PostgreSQL',
    'XJP Auth with PKCE',
    'OAuth redirect allowlist entries for every production, `www`, compatibility, Vercel/custom, and local development host that can initiate login.',
    'Start Google login from each live host and fail the release on `invalid_request` / `Invalid redirect_uri`.',
    'shared payments service',
    'Support Mail',
    'xjp-mail-service',
    'support mailbox readiness',
    'Supabase Postgres through session pooler port `5432`',
    'Rust Build Lane',
    'deploy-center approved privatecloud/codebase build lane',
    '`deploy_type=native_workflow`',
    '`source_strategy=xjp_native_codebase_runner`',
    'Do not run `cargo build`, `docker build`, or `docker compose up --build` on GCP production VMs',
    'MissionD project management must show each product-service project',
    'The canonical deployment-channel shape is build/runtime/frontend',
    'Default Vercel deployment is frontend-only.',
    'projects/<project-id>/<environment>/<SECRET_NAME>',
    'experimentalServices',
    '.missiond/v3/shards/universe/project-registry.lisp',
  ]);

  requireExistingText(diagnostics, '.missiond/templates/independent-app-bootstrap/SKILL.md', [
    'Vercel frontend plus deploy-center runtime deployment for Rust APIs',
    'Frontend Vercel + Privatecloud Rust Build Pattern',
    'Do not run `cargo build`, `docker build`, or `docker compose up --build` on GCP production VMs',
    'Rust Build Lane',
    'privatecloud-10900kf',
    'Deploy Center / Privatecloud',
  ]);

  const checkerResults = [];
  const behaviorClosureResults = [];
  const projectionDiagnostics = [];
  for (const project of projects) {
    if (!project.root) continue;
    if (!fs.existsSync(project.root)) {
      diagnostics.push({ file: project.root, message: `missing project root for ${project.id}` });
      continue;
    }
    if (project.checker) {
      const [cmd, args] = project.checker;
      const proc = spawnSync(cmd, args, { cwd: project.root, encoding: 'utf8', timeout: 60_000 });
      const ok = proc.status === 0 && !proc.error;
      checkerResults.push({
        id: project.id,
        ok,
        command: `${cmd} ${args.join(' ')}`,
        stdout_tail: tail(proc.stdout ?? ''),
        stderr_tail: tail(proc.stderr ?? ''),
        error: proc.error?.message ?? null,
      });
      if (!ok) diagnostics.push({ file: project.root, message: `project checker failed: ${cmd} ${args.join(' ')}` });
    }
    const behaviorProc = spawnSync('node', [
      'scripts/check-project-behavior-closure.mjs',
      '--project',
      project.id,
      '--root',
      project.root,
      '--json',
    ], { cwd: process.cwd(), encoding: 'utf8', timeout: 60_000 });
    const behaviorOk = behaviorProc.status === 0 && !behaviorProc.error;
    let behaviorJson = null;
    try {
      behaviorJson = JSON.parse(behaviorProc.stdout || '{}');
    } catch {
      behaviorJson = null;
    }
    behaviorClosureResults.push({
      id: project.id,
      ok: behaviorOk,
      observed_count: behaviorJson?.observed_count ?? null,
      behavior_count: behaviorJson?.behavior_count ?? null,
      effect_count: behaviorJson?.effect_count ?? null,
      projection_ok: behaviorJson?.projection_ok ?? null,
      projection_diagnostics: behaviorJson?.projection_diagnostics ?? [],
      navigation_artifact: behaviorJson?.navigation_artifact ?? null,
      diagnostics: behaviorJson?.diagnostics ?? [],
      stderr_tail: tail(behaviorProc.stderr ?? ''),
      error: behaviorProc.error?.message ?? null,
    });
    for (const diagnostic of behaviorJson?.projection_diagnostics ?? []) {
      projectionDiagnostics.push({
        project: project.id,
        file: diagnostic.file,
        line: diagnostic.line,
        column: diagnostic.column,
        code: diagnostic.code,
        message: diagnostic.message,
      });
    }
    if (!behaviorOk) {
      diagnostics.push({ file: project.root, message: `project behavior closure failed for ${project.id}` });
    }
  }

  const result = {
    ok: diagnostics.length === 0,
    engine,
    typedUniverse: {
      source: typedUniverse.source,
      project_count: typedUniverse.projects.length,
      maturity_count: typedUniverse.maturity.length,
    },
    projects: projects.map((p) => p.id),
    maturity: Object.fromEntries(typedUniverse.maturity.map((entry) => [entry.id, {
      current: entry.current,
      target: entry.target,
      gap: entry.gap,
    }])),
    checkerResults,
    behaviorClosureResults,
    projectionDiagnostics,
    diagnostics,
  };
  if (opts.json) {
    fs.writeSync(1, `${JSON.stringify(result, null, 2)}\n`);
  } else if (result.ok) {
    console.log(`project SSOT universe check OK (${projects.filter((project) => project.root).length} rooted projects/services from ${typedUniverse.source})`);
    if (projectionDiagnostics.length > 0) {
      console.error(`project behavior navigation projection has ${projectionDiagnostics.length} diagnostic(s); run node scripts/propose-behavior-navigation.mjs --project <id> --write for affected projects`);
    }
  } else {
    for (const d of diagnostics) console.error(`${d.file}: ${d.message}`);
    console.error(`project SSOT universe check FAILED -- ${diagnostics.length} diagnostic(s)`);
  }
  process.exit(result.ok ? 0 : 1);
}

function parseArgs(args) {
  const opts = { json: false, engine: 'ocaml' };
  for (let i = 0; i < args.length; i += 1) {
    const arg = args[i];
    if (arg === '--json') opts.json = true;
    else if (arg === '--help' || arg === '-h') {
      console.log(usage);
      process.exit(0);
    } else if (arg === '--engine') {
      opts.engine = args[++i] ?? fail('--engine requires a value');
    } else if (arg.startsWith('--engine=')) {
      opts.engine = arg.slice('--engine='.length);
    } else {
      fail(`unknown argument: ${arg}`);
    }
  }
  if (!['auto', 'js', 'ocaml'].includes(opts.engine)) fail(`unknown engine: ${opts.engine}`);
  return opts;
}

function fail(message) {
  console.error(`${message}\n\n${usage}`);
  process.exit(2);
}

function runOcamlUniverseCheck(engine) {
  if (engine === 'js') return { requested: engine, mode: 'js', ok: true, diagnostics: [] };
  const attempt = maybeRunLispc([
    'check-project',
    '--blueprint',
    '.missiond/v3/missiond-blueprint.lisp',
  ], { engine });
  if (attempt.mode === 'js-fallback') {
    return {
      requested: engine,
      mode: 'js-fallback',
      ok: true,
      diagnostics: attempt.result?.diagnostics ?? [],
    };
  }
  const result = attempt.result;
  if (engine === 'ocaml' && result?.unavailable) return { requested: engine, mode: 'ocaml', strictResult: result };
  return {
    requested: engine,
    mode: 'ocaml',
    ok: result?.ok === true,
    diagnostics: result?.diagnostics ?? [],
  };
}

function loadTypedUniverseProjects(engine) {
  if (engine === 'js') return loadCompiledUniverseFile();
  const result = runLispc([
    'emit-universe',
    '--blueprint',
    '.missiond/v3/missiond-blueprint.lisp',
  ]);
  if (result?.ok === true && result.compiled?.payload) {
    return normalizeTypedUniversePayload(result.compiled.payload, 'ocaml-emit-universe');
  }
  return {
    source: 'ocaml-emit-universe',
    projects: [],
    maturity: [],
    diagnostics: (result?.diagnostics ?? []).map((d) => ({
      file: d.file ?? '.missiond/v3/missiond-blueprint.lisp',
      message: `${d.code ?? 'OCAML_EMIT_UNIVERSE'}: ${d.message}`,
    })),
  };
}

function loadCompiledUniverseFile() {
  if (!fs.existsSync(COMPILED_UNIVERSE)) {
    return {
      source: COMPILED_UNIVERSE,
      projects: [],
      maturity: [],
      diagnostics: [{ file: COMPILED_UNIVERSE, message: 'missing compiled universe projection; run node scripts/compile-v3-runtime.mjs --json' }],
    };
  }
  try {
    const compiled = JSON.parse(fs.readFileSync(COMPILED_UNIVERSE, 'utf8'));
    return normalizeTypedUniversePayload(compiled.payload, COMPILED_UNIVERSE);
  } catch (error) {
    return {
      source: COMPILED_UNIVERSE,
      projects: [],
      maturity: [],
      diagnostics: [{ file: COMPILED_UNIVERSE, message: `invalid compiled universe projection: ${error.message}` }],
    };
  }
}

function normalizeTypedUniversePayload(payload, source) {
  const diagnostics = [];
  const rawProjects = Array.isArray(payload?.projects) ? payload.projects : [];
  const rawMaturity = Array.isArray(payload?.maturity) ? payload.maturity : [];
  if (rawProjects.length === 0) diagnostics.push({ file: source, message: 'typed universe projection has no projects[]' });
  if (rawMaturity.length === 0) diagnostics.push({ file: source, message: 'typed universe projection has no maturity[]' });
  const projects = rawProjects.map((project) => ({
    id: project.id,
    kind: project.kind ?? null,
    management_domain: project.management_domain ?? null,
    runtime_layer: project.runtime_layer ?? null,
    root: project.root ?? null,
    status: project.status ?? null,
    checks: Array.isArray(project.checks) ? project.checks : [],
  })).filter((project) => project.id);
  const maturity = rawMaturity.map((entry) => ({
    id: entry.id,
    current: entry.current,
    target: entry.target,
    gap: Array.isArray(entry.gap) ? entry.gap : [],
  })).filter((entry) => entry.id);
  return { source, projects, maturity, diagnostics };
}

function checkProjectTaxonomy(diagnostics, projects) {
  for (const project of projects) {
    if (!project.management_domain) {
      diagnostics.push({
        file: '.missiond/v3/shards/universe/project-registry.lisp',
        message: `project ${project.id} missing :management-domain`,
      });
    } else if (!MANAGEMENT_DOMAINS.has(project.management_domain)) {
      diagnostics.push({
        file: '.missiond/v3/shards/universe/project-registry.lisp',
        message: `project ${project.id} has unknown management domain ${project.management_domain}`,
      });
    }
    if (!project.runtime_layer) {
      diagnostics.push({
        file: '.missiond/v3/shards/universe/project-registry.lisp',
        message: `project ${project.id} missing :runtime-layer`,
      });
    } else if (!RUNTIME_LAYERS.has(project.runtime_layer)) {
      diagnostics.push({
        file: '.missiond/v3/shards/universe/project-registry.lisp',
        message: `project ${project.id} has unknown runtime layer ${project.runtime_layer}`,
      });
    }
  }
}

function checkMaturityRegistry(diagnostics, maturityEntries) {
  const maturity = Object.fromEntries(maturityEntries.map((entry) => [entry.id, entry]));
  const expectedIds = ['missiond', 'board', ...maturityEntries.map((entry) => entry.id).filter((id) => id !== 'missiond' && id !== 'board')];
  for (const id of expectedIds) {
    const entry = maturity[id];
    if (!entry) {
      diagnostics.push({ file: '.missiond/v3/missiond-blueprint.lisp', message: `missing maturity registry entry for ${id}` });
      continue;
    }
    if (entry.target !== 'M6') {
      diagnostics.push({ file: '.missiond/v3/missiond-blueprint.lisp', message: `${id} target must be M6, got ${entry.target}` });
    }
    if (entry.current !== 'M6' && entry.gap.length === 0) {
      diagnostics.push({ file: '.missiond/v3/missiond-blueprint.lisp', message: `${id} is not M6 but has no maturity gap` });
    }
  }
}

function checkProjectMaturityCoverage(diagnostics, projects, maturityEntries) {
  const maturityIds = new Set(maturityEntries.map((entry) => entry.id));
  for (const project of projects) {
    if (project.status === 'runtime-registered') continue;
    if (!maturityIds.has(project.id)) {
      diagnostics.push({
        file: '.missiond/v3/missiond-blueprint.lisp',
        message: `typed project ${project.id} has no maturity registry entry`,
      });
    }
  }
}

function requireAll(diagnostics, file, source, needles) {
  for (const needle of needles) {
    if (!source.includes(needle)) diagnostics.push({ file, message: `missing required text: ${needle}` });
  }
}

function requireExistingText(diagnostics, file, needles) {
  if (!fs.existsSync(file)) {
    diagnostics.push({ file, message: 'missing required path' });
    return;
  }
  const source = fs.readFileSync(file, 'utf8');
  requireAll(diagnostics, file, source, needles);
}

function tail(text, lines = 6) {
  return text ? text.split('\n').slice(-lines).join('\n').trim() : '';
}

main();
