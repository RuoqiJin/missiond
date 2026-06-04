#!/usr/bin/env node

import { spawnSync } from 'node:child_process';
import fs from 'node:fs';

const DEFAULT_DEPLOY_BASE_URL = process.env.DEPLOY_CENTER_PUBLIC_BASE_URL || 'https://deploy.xiaojins.com';
const DEFAULT_TIMEOUT_MS = Number(process.env.DEPLOY_CHAIN_AUDIT_TIMEOUT_MS || 8000);
const PROJECT_UNIVERSE_PATH = '.missiond/v3/runtime/compiled/compiled-project-universe.json';
const DEFAULT_READ_TOKEN_REF = 'secret-store://missiond/production/MISSIOND_DEPLOY_CENTER_READ_TOKEN';
const DEFAULT_WRITE_TOKEN_REF = 'deploy/ci_deploy_api_key';
const MISSIOND_ROOT = '/Users/jinchen/Projects/missiond';
const DEPLOY_CENTER_REPO_ROOT = '/Users/jinchen/Downloads/xiaojinpro-gateway/xiaojinpro-backend';
const DEPLOY_CENTER_DIRECT_PATHS = [
  '.github/workflows/deploy-center.yml',
  'docker/Dockerfile.deploy-center',
  'services/deploy-center',
];
const DEPLOY_CENTER_CARGO_ROOT_PATHS = ['Cargo.toml', 'Cargo.lock'];
const DEPLOY_CENTER_CARGO_RELEVANCE_RE = /deploy[-_]center|xjp[-_]deploy[-_]center|services\/deploy-center/i;

const TARGETS = [
  {
    id: 'deploy-center-self-update',
    kind: 'self_update',
    serviceId: 'deploy-center',
    deploySlug: 'xjp-deploy-center',
    healthUrls: ['https://deploy.xiaojins.com/api/deploy/health'],
  },
  {
    id: 'pay',
    kind: 'service',
    serviceId: 'payments',
    deploySlug: 'xjp-payments',
    healthUrls: ['https://pay.xiaojins.com/health', 'https://pay.xiaojins.com/payments/health'],
  },
  {
    id: 'search',
    kind: 'service',
    serviceId: 'search-center',
    deploySlug: 'xjp-search-center',
    healthUrls: ['https://search-center.xiaojins.com/v1/health'],
  },
  {
    id: 'legal',
    kind: 'service',
    serviceId: 'xjp-legal-service',
    deploySlug: 'xjp-legal-service',
    healthUrls: ['https://legal.xiaojins.com/health'],
  },
  {
    id: 'project-universe',
    kind: 'service',
    serviceId: 'xjp-project-universe',
    deploySlug: 'xjp-project-universe',
    healthUrls: ['https://projects.xiaojins.com/health'],
  },
  {
    id: 'domain-route',
    kind: 'service',
    serviceId: 'xjp-domain-service',
    deploySlug: 'xjp-domain-service',
    healthUrls: ['https://domains.xiaojins.com/health'],
  },
];

function parseArgs(argv) {
  const opts = {
    json: false,
    strict: false,
    includeConversationAudit: false,
    deployBaseUrl: DEFAULT_DEPLOY_BASE_URL,
    timeoutMs: DEFAULT_TIMEOUT_MS,
  };
  for (let i = 2; i < argv.length; i += 1) {
    const arg = argv[i];
    if (arg === '--json') opts.json = true;
    else if (arg === '--strict') opts.strict = true;
    else if (arg === '--include-conversation-audit') opts.includeConversationAudit = true;
    else if (arg === '--base-url') opts.deployBaseUrl = argv[++i] || fail('--base-url requires a value');
    else if (arg.startsWith('--base-url=')) opts.deployBaseUrl = arg.slice('--base-url='.length);
    else if (arg === '--timeout-ms') opts.timeoutMs = Number(argv[++i] || fail('--timeout-ms requires a value'));
    else if (arg.startsWith('--timeout-ms=')) opts.timeoutMs = Number(arg.slice('--timeout-ms='.length));
    else if (arg === '--help' || arg === '-h') {
      usage();
      process.exit(0);
    } else {
      fail(`unknown argument: ${arg}`);
    }
  }
  if (!Number.isFinite(opts.timeoutMs) || opts.timeoutMs <= 0) {
    fail('--timeout-ms must be a positive number');
  }
  return opts;
}

function usage() {
  console.log(`Usage:
  node scripts/audit-deploy-chain-closure.mjs [--json] [--strict] [--include-conversation-audit] [--base-url URL]

Read-only audit for the deployment chain exposed by the 2026-06-04
Codex/Deploy Center/MissionD incident. It does not trigger deployments.
It checks the ordered lane:
  deploy-center-self-update -> pay -> search -> legal -> project-universe -> domain-route

The audit separates public route health from Deploy Center provenance closure.`);
}

function fail(message) {
  console.error(message);
  process.exit(2);
}

function command(cmd, args, opts = {}) {
  const result = spawnSync(cmd, args, {
    encoding: 'utf8',
    maxBuffer: 32 * 1024 * 1024,
    ...opts,
  });
  return {
    ok: result.status === 0,
    status: result.status,
    stdout: result.stdout || '',
    stderr: result.stderr || '',
  };
}

function envPresence() {
  return {
    DEPLOY_CENTER_API_KEY: Boolean(process.env.DEPLOY_CENTER_API_KEY),
    DEPLOY_CENTER_ADMIN_TOKEN: Boolean(process.env.DEPLOY_CENTER_ADMIN_TOKEN),
    MISSIOND_DEPLOY_CENTER_READ_TOKEN: Boolean(process.env.MISSIOND_DEPLOY_CENTER_READ_TOKEN),
    MISSIOND_DEPLOY_CENTER_READ_TOKEN_REF: Boolean(process.env.MISSIOND_DEPLOY_CENTER_READ_TOKEN_REF),
    DEPLOY_CENTER_API_KEY_REF: Boolean(process.env.DEPLOY_CENTER_API_KEY_REF),
    DEPLOY_CENTER_ADMIN_TOKEN_REF: Boolean(process.env.DEPLOY_CENTER_ADMIN_TOKEN_REF),
    SECRET_STORE_API_KEY: Boolean(process.env.SECRET_STORE_API_KEY),
    XJP_API_KEY: Boolean(process.env.XJP_API_KEY),
  };
}

function credentialRefSummary(env) {
  const writeRef = process.env.DEPLOY_CENTER_API_KEY_REF || process.env.DEPLOY_CENTER_ADMIN_TOKEN_REF || DEFAULT_WRITE_TOKEN_REF;
  const explicitWriteRef = Boolean(process.env.DEPLOY_CENTER_API_KEY_REF || process.env.DEPLOY_CENTER_ADMIN_TOKEN_REF);
  const secretStoreAccessPresent = env.SECRET_STORE_API_KEY || env.XJP_API_KEY;
  return {
    readToken: {
      env: 'MISSIOND_DEPLOY_CENTER_READ_TOKEN',
      present: env.MISSIOND_DEPLOY_CENTER_READ_TOKEN,
      refEnv: 'MISSIOND_DEPLOY_CENTER_READ_TOKEN_REF',
      refPresent: env.MISSIOND_DEPLOY_CENTER_READ_TOKEN_REF,
      ref: process.env.MISSIOND_DEPLOY_CENTER_READ_TOKEN_REF || DEFAULT_READ_TOKEN_REF,
      defaultRefDeclared: true,
    },
    writeToken: {
      acceptedEnv: ['DEPLOY_CENTER_API_KEY', 'DEPLOY_CENTER_ADMIN_TOKEN'],
      present: env.DEPLOY_CENTER_API_KEY || env.DEPLOY_CENTER_ADMIN_TOKEN,
      refEnv: ['DEPLOY_CENTER_API_KEY_REF', 'DEPLOY_CENTER_ADMIN_TOKEN_REF'],
      refPresent: explicitWriteRef,
      ref: writeRef,
      defaultRefDeclared: true,
      secretStoreAccessEnv: ['SECRET_STORE_API_KEY', 'XJP_API_KEY'],
      secretStoreAccessPresent,
      diagnostic: (env.DEPLOY_CENTER_API_KEY || env.DEPLOY_CENTER_ADMIN_TOKEN)
        ? null
        : 'write token value is not injected; resolving the Secret Store ref requires valid SECRET_STORE_API_KEY or XJP_API_KEY',
    },
  };
}

function repoState(cwd, pathspecs = []) {
  if (!fs.existsSync(cwd)) return { exists: false };
  const head = command('git', ['rev-parse', 'HEAD'], { cwd });
  const branch = command('git', ['branch', '--show-current'], { cwd });
  const status = command('git', ['status', '--porcelain=v1'], { cwd });
  const relevantStatus = pathspecs.length > 0
    ? command('git', ['status', '--porcelain=v1', '--', ...pathspecs], { cwd })
    : status;
  const upstream = command('git', ['rev-parse', '--abbrev-ref', '--symbolic-full-name', '@{u}'], { cwd });
  const upstreamRef = upstream.ok ? upstream.stdout.trim() : null;
  const aheadBehind = upstreamRef
    ? command('git', ['rev-list', '--left-right', '--count', `HEAD...${upstreamRef}`], { cwd })
    : { ok: false, stdout: '' };
  const [aheadCount, behindCount] = aheadBehind.ok
    ? aheadBehind.stdout.trim().split(/\s+/).map((value) => Number(value))
    : [null, null];
  const relevantUnpushed = upstreamRef && pathspecs.length > 0
    ? command('git', ['diff', '--name-only', `${upstreamRef}..HEAD`, '--', ...pathspecs], { cwd })
    : { ok: false, stdout: '' };
  const unpushedRelevantLines = upstreamRef && pathspecs.length > 0 && relevantUnpushed.ok
    ? relevantUnpushed.stdout.trim().split('\n').filter(Boolean).slice(0, 40)
    : [];
  const remote = command('git', ['remote', 'get-url', 'origin'], { cwd });
  const relevantDirty = relevantStatus.ok ? relevantStatus.stdout.trim().length > 0 : null;
  const relevantAhead = pathspecs.length > 0
    ? unpushedRelevantLines.length > 0
    : Number(aheadCount || 0) > 0;
  return {
    exists: true,
    head: head.ok ? head.stdout.trim() : null,
    branch: branch.ok ? branch.stdout.trim() : null,
    upstream: upstreamRef,
    origin: remote.ok ? remote.stdout.trim() : null,
    aheadCount: Number.isFinite(aheadCount) ? aheadCount : null,
    behindCount: Number.isFinite(behindCount) ? behindCount : null,
    dirty: status.ok ? status.stdout.trim().length > 0 : null,
    relevantPathspecs: pathspecs,
    relevantDirty,
    relevantAhead,
    relevantUnpublished: relevantDirty === true || relevantAhead,
    relevantDirtyLines: relevantStatus.ok ? relevantStatus.stdout.trim().split('\n').filter(Boolean).slice(0, 40) : [],
    relevantUnpushedLines: unpushedRelevantLines,
    dirtyLines: status.ok ? status.stdout.trim().split('\n').filter(Boolean).slice(0, 40) : [],
  };
}

function diffForPathspecs(cwd, pathspecs, range = null) {
  const args = ['diff'];
  if (range) args.push(range);
  args.push('--', ...pathspecs);
  return command('git', args, { cwd });
}

function deployCenterCargoRootRelevance(cwd, upstreamRef) {
  const dirtyDiff = diffForPathspecs(cwd, DEPLOY_CENTER_CARGO_ROOT_PATHS);
  const unpushedDiff = upstreamRef
    ? diffForPathspecs(cwd, DEPLOY_CENTER_CARGO_ROOT_PATHS, `${upstreamRef}..HEAD`)
    : { ok: false, stdout: '', stderr: '' };
  const diffText = `${dirtyDiff.stdout}\n${unpushedDiff.stdout}`;
  const relevant = DEPLOY_CENTER_CARGO_RELEVANCE_RE.test(diffText);
  return {
    pathspecs: DEPLOY_CENTER_CARGO_ROOT_PATHS,
    relevant,
    reason: relevant
      ? 'cargo_diff_mentions_deploy_center'
      : 'cargo_diff_does_not_mention_deploy_center',
    dirtyDiffChecked: dirtyDiff.ok,
    unpushedDiffChecked: unpushedDiff.ok,
  };
}

function deployCenterRepoState(cwd) {
  const direct = repoState(cwd, DEPLOY_CENTER_DIRECT_PATHS);
  if (!direct.exists) return direct;

  const cargo = repoState(cwd, DEPLOY_CENTER_CARGO_ROOT_PATHS);
  const cargoRelevance = deployCenterCargoRootRelevance(cwd, direct.upstream);
  const cargoBlocks = cargo.relevantUnpublished === true && cargoRelevance.relevant;
  const ignoredDirtyLines = cargoBlocks ? [] : cargo.relevantDirtyLines;
  const ignoredUnpushedLines = cargoBlocks ? [] : cargo.relevantUnpushedLines;

  return {
    ...direct,
    relevantPathspecs: [...DEPLOY_CENTER_CARGO_ROOT_PATHS, ...DEPLOY_CENTER_DIRECT_PATHS],
    relevantDirty: direct.relevantDirty === true || cargoBlocks,
    relevantAhead: direct.relevantAhead === true || cargoBlocks,
    relevantUnpublished: direct.relevantUnpublished === true || cargoBlocks,
    relevantDirtyLines: [
      ...direct.relevantDirtyLines,
      ...(cargoBlocks ? cargo.relevantDirtyLines : []),
    ],
    relevantUnpushedLines: [
      ...direct.relevantUnpushedLines,
      ...(cargoBlocks ? cargo.relevantUnpushedLines : []),
    ],
    ignoredDirtyLines,
    ignoredUnpushedLines,
    sourceRelevance: {
      direct: {
        pathspecs: DEPLOY_CENTER_DIRECT_PATHS,
        relevantDirty: direct.relevantDirty,
        relevantAhead: direct.relevantAhead,
        relevantUnpublished: direct.relevantUnpublished,
      },
      cargoRoot: {
        ...cargoRelevance,
        relevantDirty: cargo.relevantDirty,
        relevantAhead: cargo.relevantAhead,
        relevantUnpublished: cargo.relevantUnpublished,
        blocksDeployCenterSource: cargoBlocks,
        dirtyLines: cargo.relevantDirtyLines,
        unpushedLines: cargo.relevantUnpushedLines,
      },
    },
  };
}

function controlPlaneCredentialProbeConfig(env) {
  if (env.DEPLOY_CENTER_API_KEY) {
    return {
      kind: 'api_key',
      header: 'X-API-Key',
      value: process.env.DEPLOY_CENTER_API_KEY,
    };
  }
  if (env.DEPLOY_CENTER_ADMIN_TOKEN) {
    return {
      kind: 'admin_token',
      header: 'Authorization',
      value: `Bearer ${process.env.DEPLOY_CENTER_ADMIN_TOKEN}`,
    };
  }
  return null;
}

async function validateControlPlaneCredential(env, opts) {
  const probe = controlPlaneCredentialProbeConfig(env);
  if (!probe) return { checked: false, ok: false, reason: 'missing' };

  const controller = new AbortController();
  const timer = setTimeout(() => controller.abort(), opts.timeoutMs);
  const url = `${opts.deployBaseUrl.replace(/\/$/, '')}/api/deploy/codebase/overview?limit=1`;
  const started = Date.now();
  try {
    const res = await fetch(url, {
      method: 'GET',
      headers: { [probe.header]: probe.value },
      signal: controller.signal,
    });
    const bodyText = await res.text();
    return {
      checked: true,
      ok: res.ok,
      kind: probe.kind,
      url,
      status: res.status,
      durationMs: Date.now() - started,
      diagnostic: res.ok ? null : `control_plane_auth_http_${res.status}`,
      bodyPreview: res.ok ? null : bodyText.slice(0, 160),
    };
  } catch (err) {
    return {
      checked: true,
      ok: false,
      kind: probe.kind,
      url,
      status: null,
      durationMs: Date.now() - started,
      diagnostic: err instanceof Error ? err.message : String(err),
      bodyPreview: null,
    };
  } finally {
    clearTimeout(timer);
  }
}

async function localSelfUpdatePreflight(opts) {
  const script = '/Users/jinchen/Downloads/xiaojinpro-gateway/xiaojinpro-backend/services/deploy-center/scripts/run-missiond-macmini-self-update.sh';
  const scriptSyntax = fs.existsSync(script) ? command('bash', ['-n', script]) : { ok: false, stderr: 'script missing' };
  const env = envPresence();
  const hasWriteCredential = env.DEPLOY_CENTER_API_KEY || env.DEPLOY_CENTER_ADMIN_TOKEN;
  const credentialProbe = await validateControlPlaneCredential(env, opts);
  const missiond = repoState(MISSIOND_ROOT);
  const backend = deployCenterRepoState(DEPLOY_CENTER_REPO_ROOT);
  const blockers = [];
  if (!hasWriteCredential) blockers.push('deploy_center_write_token_missing');
  else if (!credentialProbe.ok) blockers.push('deploy_center_write_token_invalid');
  if (!scriptSyntax.ok) blockers.push('self_update_script_syntax_failed');
  if (missiond.relevantUnpublished) blockers.push('missiond_source_not_published');
  if (backend.relevantUnpublished) blockers.push('deploy_center_source_not_published');
  return {
    ok: blockers.length === 0,
    blockers,
    env,
    credentialRefs: credentialRefSummary(env),
    credentialProbe,
    script: { path: script, syntaxOk: scriptSyntax.ok, stderr: scriptSyntax.stderr.trim() || null },
    missiondRepo: missiond,
    deployCenterRepo: backend,
  };
}

async function fetchJson(url, timeoutMs) {
  const controller = new AbortController();
  const timer = setTimeout(() => controller.abort(), timeoutMs);
  const started = Date.now();
  try {
    const res = await fetch(url, { signal: controller.signal });
    const text = await res.text();
    let body = null;
    try {
      body = text ? JSON.parse(text) : null;
    } catch {
      body = { parse_error: true, text: text.slice(0, 500) };
    }
    return {
      url,
      ok: res.ok,
      status: res.status,
      durationMs: Date.now() - started,
      body,
    };
  } catch (err) {
    return {
      url,
      ok: false,
      status: null,
      durationMs: Date.now() - started,
      error: err instanceof Error ? err.message : String(err),
    };
  } finally {
    clearTimeout(timer);
  }
}

function provenanceUrl(baseUrl, slug) {
  return `${baseUrl.replace(/\/$/, '')}/api/deploy/provenance/${encodeURIComponent(slug)}`;
}

function loadProjectUniverse() {
  try {
    const parsed = JSON.parse(fs.readFileSync(PROJECT_UNIVERSE_PATH, 'utf8'));
    return {
      ok: parsed.schema_version === 'missiond.compiled-project-universe.v1',
      path: PROJECT_UNIVERSE_PATH,
      schemaVersion: parsed.schema_version || null,
      sourceHash: parsed.source_hash || null,
      payload: parsed.payload || {},
      diagnostics: parsed.diagnostics || [],
    };
  } catch (err) {
    return {
      ok: false,
      path: PROJECT_UNIVERSE_PATH,
      error: err instanceof Error ? err.message : String(err),
      payload: {},
      diagnostics: [],
    };
  }
}

function hostnames(urls) {
  const names = new Set();
  for (const url of urls) {
    try {
      names.add(new URL(url).hostname);
    } catch {
      // ignore non-URL entries
    }
  }
  return [...names].sort();
}

function findService(projectUniverse, target) {
  const services = projectUniverse.payload.services || [];
  return services.find((service) => {
    return service.id === target.serviceId
      || service.project === target.serviceId
      || service.deployment?.dc_slug === target.deploySlug
      || service.support_catalog?.deploy_center_slug === target.deploySlug
      || (service.deployment_channels || []).some((channel) => channel.deploy_center_slug === target.deploySlug);
  }) || null;
}

function findDomainRows(projectUniverse, service, expectedDomains) {
  const domains = projectUniverse.payload.domain_management?.domains || [];
  if (!service) return [];
  const serviceIds = new Set([service.id, service.project, service.support_catalog?.service_id, service.support_catalog?.project_id].filter(Boolean));
  return domains.filter((row) => {
    const rowDomain = row.domain;
    const matchesDomain = rowDomain && expectedDomains.includes(rowDomain);
    const matchesOwner = serviceIds.has(row.owner_service_id)
      || serviceIds.has(row.owner_project_id)
      || serviceIds.has(row.proxy_intent?.service_id)
      || serviceIds.has(row.proxy_intent?.project_id)
      || (row.declared_records || []).some((record) => serviceIds.has(record.service_id));
    return matchesDomain || matchesOwner;
  });
}

function summarizeDeploymentChannels(service) {
  const channels = service?.deployment_channels || [];
  return channels.map((channel) => ({
    id: channel.id || null,
    surface: channel.surface || null,
    channelKind: channel.channel_kind || null,
    runnerRole: channel.runner_role || null,
    executor: channel.executor || null,
    builder: channel.builder || null,
    deployCenterSlug: channel.deploy_center_slug || null,
    runtimeTarget: channel.runtime_target || null,
    targetSideBuildProhibited: channel.target_side_build_prohibited ?? null,
    manifest: channel.manifest || null,
    sourceRef: channel.source_ref || null,
  }));
}

function auditConfigClosure(projectUniverse, target) {
  const service = findService(projectUniverse, target);
  const expectedDomains = hostnames(target.healthUrls);
  const domainRows = findDomainRows(projectUniverse, service, expectedDomains);
  const channels = service?.deployment_channels || [];
  const buildChannel = channels.find((channel) => channel.surface === 'build');
  const runtimeChannel = channels.find((channel) => channel.surface === 'runtime');
  const diagnostics = [];

  if (!projectUniverse.ok) diagnostics.push('compiled_project_universe_unavailable');
  if (!service) diagnostics.push('service_runtime_config_missing');
  if (service && service.deployment?.dc_slug !== target.deploySlug) diagnostics.push('deploy_center_slug_mismatch');
  if (!buildChannel && !service?.build_lane) diagnostics.push('build_lane_missing');
  if (!runtimeChannel && !service?.deployment) diagnostics.push('runtime_lane_missing');
  if ((buildChannel?.runner_role || null) !== 'build_runner') diagnostics.push('build_runner_role_missing');
  if ((runtimeChannel?.runner_role || null) !== 'runtime_runner') diagnostics.push('runtime_runner_role_missing');
  if (buildChannel && buildChannel.executor === 'gcp-agent') diagnostics.push('gcp_build_forbidden');
  if (buildChannel && buildChannel.target_side_build_prohibited !== true) diagnostics.push('target_side_build_not_prohibited');
  if (!service?.proxy && !service?.domain_proxy_intent) diagnostics.push('caddy_proxy_intent_missing');
  if (expectedDomains.length > 0 && domainRows.length === 0) diagnostics.push('domain_management_binding_missing');

  const declaredRecords = domainRows.flatMap((row) => row.declared_records || []);
  const missingRecords = expectedDomains.filter((domain) => !declaredRecords.some((record) => record.name === domain));
  if (expectedDomains.length > 0 && missingRecords.length > 0) {
    diagnostics.push(`dns_record_missing:${missingRecords.join('|')}`);
  }

  return {
    status: diagnostics.length === 0 ? 'closed' : 'config_gap',
    closed: diagnostics.length === 0,
    diagnostic: diagnostics.length === 0 ? null : diagnostics.join(', '),
    projectUniverse: {
      path: projectUniverse.path,
      schemaVersion: projectUniverse.schemaVersion || null,
      sourceHash: projectUniverse.sourceHash || null,
    },
    service: service ? {
      id: service.id || null,
      project: service.project || null,
      deploySlug: service.deployment?.dc_slug || service.support_catalog?.deploy_center_slug || null,
      canonicalDomain: service.canonical_domain || null,
      domains: service.domains || [],
      runtimeTarget: service.deployment?.runtime_target || null,
      runtimeExecutor: service.deployment?.executor || null,
      buildExecutor: buildChannel?.executor || service.build_lane?.executor || null,
      builder: buildChannel?.builder || service.build_lane?.builder || null,
      buildImage: buildChannel?.image || service.build_lane?.image || null,
      proxy: service.proxy ? {
        kind: service.proxy.kind || null,
        domain: service.proxy.domain || null,
        upstream: service.proxy.upstream || null,
        routes: service.proxy.routes || [],
      } : null,
      domainProxyIntent: service.domain_proxy_intent ? {
        kind: service.domain_proxy_intent.kind || null,
        domain: service.domain_proxy_intent.domain || null,
        upstream: service.domain_proxy_intent.upstream || null,
        health: service.domain_proxy_intent.health || [],
        activeRequires: service.domain_proxy_intent.lifecycle?.active_requires || [],
      } : null,
      deploymentChannels: summarizeDeploymentChannels(service),
    } : null,
    domainManagement: domainRows.map((row) => ({
      domain: row.domain || null,
      managedBy: row.managed_by || null,
      ownerProjectId: row.owner_project_id || null,
      ownerServiceId: row.owner_service_id || null,
      recordNames: (row.declared_records || []).map((record) => record.name).filter(Boolean),
      proxyIntent: row.proxy_intent ? {
        kind: row.proxy_intent.kind || null,
        domain: row.proxy_intent.domain || null,
        upstream: row.proxy_intent.upstream || null,
        health: row.proxy_intent.health || [],
      } : null,
    })),
  };
}

function classifyProvenance(result) {
  if (!result.ok) {
    return {
      status: 'unreachable',
      closed: false,
      diagnostic: `provenance_http_${result.status || 'error'}`,
    };
  }
  const body = result.body || {};
  const complete = body.provenance_complete === true || body.status === 'closed';
  const confidence = String(body.confidence || '').toLowerCase();
  if (complete && (!confidence || confidence === 'high')) {
    return { status: 'closed', closed: true, diagnostic: null };
  }
  if (body.status === 'unknown' || confidence === 'none') {
    return { status: 'missing', closed: false, diagnostic: 'deploy_center_provenance_missing' };
  }
  return { status: 'provenance_partial', closed: false, diagnostic: 'deploy_center_provenance_partial' };
}

function classifyRoute(healthResults) {
  const failed = healthResults.filter((row) => !row.ok);
  if (failed.length === 0) return { status: 'healthy', healthy: true, diagnostic: null };
  return {
    status: 'route_unhealthy',
    healthy: false,
    diagnostic: failed.map((row) => `${row.url}:http_${row.status || 'error'}`).join('; '),
  };
}

async function auditTarget(target, opts, projectUniverse) {
  const provenance = await fetchJson(provenanceUrl(opts.deployBaseUrl, target.deploySlug), opts.timeoutMs);
  const health = [];
  for (const url of target.healthUrls) health.push(await fetchJson(url, opts.timeoutMs));
  const provenanceState = classifyProvenance(provenance);
  const routeState = classifyRoute(health);
  const configState = auditConfigClosure(projectUniverse, target);
  const closed = configState.closed && provenanceState.closed && routeState.healthy;
  return {
    id: target.id,
    kind: target.kind,
    serviceId: target.serviceId,
    deploySlug: target.deploySlug,
    closure: closed ? 'closed' : (!configState.closed ? configState.status : provenanceState.status),
    canContinuePast: closed,
    config: configState,
    provenance: {
      ...provenanceState,
      httpStatus: provenance.status,
      url: provenance.url,
      body: summarizeProvenanceBody(provenance.body),
    },
    route: {
      ...routeState,
      health,
    },
  };
}

function summarizeProvenanceBody(body) {
  if (!body || typeof body !== 'object') return body ?? null;
  return {
    status: body.status ?? null,
    confidence: body.confidence ?? null,
    provenance_complete: body.provenance_complete ?? null,
    legacy_row: body.legacy_row ?? null,
    deployed_commit: body.deployed_commit ?? body.commit_hash ?? null,
    target_image: body.target_image ?? null,
    target_digest: body.target_digest ?? body.reported_digest ?? null,
    diagnostics: body.diagnostics ?? body.diagnostic ?? null,
  };
}

function runConversationAudit() {
  const result = command(process.execPath, ['scripts/audit-codex-history-ingestion.mjs', '--json'], {
    cwd: process.cwd(),
  });
  if (!result.ok) {
    return { ok: false, error: result.stderr || result.stdout };
  }
  try {
    const parsed = JSON.parse(result.stdout);
    return {
      ok: parsed.ok === true,
      raw: parsed.raw,
      missiond: parsed.missiond,
      missingInMissionD: parsed.missingInMissionD?.length ?? null,
      rawRolloutsMissingInMissionD: parsed.rawRolloutsMissingInMissionD?.length ?? null,
    };
  } catch (err) {
    return { ok: false, error: `parse failed: ${err.message}` };
  }
}

function firstBlockedStep(preflight, targets) {
  if (!preflight.ok) {
    return {
      id: 'deploy-center-self-update',
      reason: preflight.blockers.join(', '),
    };
  }
  const blocked = targets.find((target) => !target.canContinuePast);
  if (!blocked) return null;
  return {
    id: blocked.id,
    reason: blocked.provenance.diagnostic || blocked.route.diagnostic || blocked.closure,
  };
}

function printHuman(report) {
  console.log(`Deploy chain closure audit: ${report.ok ? 'OK' : 'NOT CLOSED'}`);
  if (report.firstBlockedStep) {
    console.log(`First blocked step: ${report.firstBlockedStep.id} (${report.firstBlockedStep.reason})`);
  }
  for (const target of report.targets) {
    console.log(
      `- ${target.id}: route=${target.route.status} provenance=${target.provenance.status} closure=${target.closure}`,
    );
  }
  if (report.conversationAudit) {
    console.log(`Conversation audit: ${report.conversationAudit.ok ? 'OK' : 'NEEDS ATTENTION'}`);
  }
}

async function main() {
  const opts = parseArgs(process.argv);
  const preflight = await localSelfUpdatePreflight(opts);
  const projectUniverse = loadProjectUniverse();
  const targets = [];
  for (const target of TARGETS) targets.push(await auditTarget(target, opts, projectUniverse));
  const conversationAudit = opts.includeConversationAudit ? runConversationAudit() : null;
  const firstBlocked = firstBlockedStep(preflight, targets);
  const report = {
    schema: 'missiond.deploy-chain-closure-audit.v1',
    generatedAt: new Date().toISOString(),
    deployBaseUrl: opts.deployBaseUrl,
    orderedLane: TARGETS.map((target) => target.id),
    ok: !firstBlocked && (!conversationAudit || conversationAudit.ok),
    firstBlockedStep: firstBlocked,
    preflight,
    targets,
    conversationAudit,
  };

  if (opts.json) console.log(JSON.stringify(report, null, 2));
  else printHuman(report);
  process.exit(report.ok || !opts.strict ? 0 : 1);
}

main().catch((err) => {
  console.error(err instanceof Error ? err.stack || err.message : String(err));
  process.exit(1);
});
