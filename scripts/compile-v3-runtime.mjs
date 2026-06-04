#!/usr/bin/env node

import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { runLispc } from './lib/ocaml_lispc.mjs';
import { buildAgentSlices, buildProjectAgentNavigation } from './lib/v3_agent_slices.mjs';
import { runSemanticRules } from './lib/v3_semantic_rules.mjs';
import { RUNTIME_DOMAIN_SPECS } from './lib/v3_runtime_domains.mjs';
import { augmentWorkstationProviderLaunchPolicy } from './lib/v3_workstation_provider_launch_policy.mjs';
import { generateBehaviorNavigation } from './propose-behavior-navigation.mjs';

const LEGACY_REPO_OUT_DIR = '.missiond/v3/runtime/compiled';
const OUT_DIR = process.env.MISSIOND_RUNTIME_DIR
  ? path.join(process.env.MISSIOND_RUNTIME_DIR, 'compiled')
  : LEGACY_REPO_OUT_DIR;
const BLUEPRINT = '.missiond/v3/missiond-blueprint.lisp';
const WORKFLOW_DIR = '.missiond/workflows';
const GENOME_DIR = '.missiond/v3/genome';

const targets = [
  {
    id: 'v3',
    argv: ['emit-v3', '--blueprint', BLUEPRINT],
    file: 'compiled-v3-blueprint.json',
  },
  {
    id: 'runtime-config',
    argv: ['emit-runtime-config', '--blueprint', BLUEPRINT],
    file: 'compiled-runtime-config.json',
  },
  {
    id: 'semantic-ir',
    argv: ['emit-semantic-ir', '--blueprint', BLUEPRINT],
    file: 'compiled-semantic-ir.json',
  },
  {
    id: 'contract-abi',
    argv: ['emit-contract-abi', '--blueprint', BLUEPRINT],
    file: 'compiled-contract-abi.json',
  },
  {
    id: 'universe',
    argv: ['emit-universe', '--blueprint', BLUEPRINT],
    file: 'compiled-project-universe.json',
  },
  {
    id: 'workflows',
    argv: ['emit-workflows', '--workflow-dir', WORKFLOW_DIR],
    file: 'compiled-workflows.json',
  },
  {
    id: 'genomes',
    argv: ['emit-genomes', '--genome-dir', GENOME_DIR],
    file: 'compiled-genomes.json',
  },
];

function main() {
  const opts = parseArgs(process.argv.slice(2));
  const outDir = opts.check && opts.outDir === OUT_DIR
    ? fs.mkdtempSync(path.join(os.tmpdir(), 'missiond-v3-runtime-check-'))
    : opts.outDir;
  fs.mkdirSync(outDir, { recursive: true });
  const results = [];
  const bundle = runLispc([
    'compile-v3-runtime',
    '--blueprint',
    BLUEPRINT,
    '--workflow-dir',
    WORKFLOW_DIR,
    '--genome-dir',
    GENOME_DIR,
  ]);
  const bundleTargets = bundle?.compiled?.payload?.targets ?? {};
  if (!bundle?.compiled || !bundleTargets || typeof bundleTargets !== 'object') {
    results.push({
      id: 'compile-v3-runtime-bundle',
      ok: false,
      diagnostics: bundle?.diagnostics ?? [],
      stderr: bundle?.stderr ?? '',
    });
  }
  for (const target of targets) {
    let compiled = bundleTargets[target.id];
    if (!compiled || !targetCompiledOk(compiled)) {
      results.push({
        id: target.id,
        ok: false,
        diagnostics: compiled?.diagnostics ?? bundle?.diagnostics ?? [],
        stderr: bundle?.stderr ?? '',
      });
      continue;
    }
    if (target.id === 'universe') {
      compiled = augmentProjectUniverseDeploymentChannels(compiled);
      compiled = augmentProjectUniverseDomainProxyLifecycle(compiled);
      compiled = augmentProjectUniverseDomainManagement(compiled);
    }
    if (target.id === 'runtime-config') {
      augmentWorkstationProviderLaunchPolicy(compiled.payload, { repo: process.cwd() });
    }
    const outPath = path.join(outDir, target.file);
    fs.writeFileSync(outPath, `${JSON.stringify(compiled, null, 2)}\n`);
    results.push({ id: target.id, ok: true, path: outPath, source_hash: compiled.source_hash });
  }
  const runtimeConfig = bundleTargets['runtime-config'];
  if (runtimeConfig && targetCompiledOk(runtimeConfig)) {
    const domainTargets = compiledRuntimeDomainTargets(runtimeConfig);
    for (const domainTarget of domainTargets) {
      if (!domainTarget.ok) {
        results.push(domainTarget);
        continue;
      }
      const outPath = path.join(outDir, domainTarget.file);
      if (domainTarget.domain === 'workstation') {
        augmentWorkstationProviderLaunchPolicy(domainTarget.compiled.payload, {
          repo: process.cwd(),
        });
      }
      fs.writeFileSync(outPath, `${JSON.stringify(domainTarget.compiled, null, 2)}\n`);
      results.push({
        id: `runtime-domain:${domainTarget.domain}`,
        ok: true,
        path: outPath,
        source_hash: domainTarget.compiled.source_hash,
      });
    }
  }
  const contractAbi = bundleTargets['contract-abi'];
  if (contractAbi && targetCompiledOk(contractAbi)) {
    const finalManifest = compiledFinalConvergenceManifest(contractAbi);
    const outPath = path.join(outDir, 'compiled-final-convergence-manifest.json');
    fs.writeFileSync(outPath, `${JSON.stringify(finalManifest, null, 2)}\n`);
    results.push({
      id: 'final-convergence-manifest',
      ok: finalManifest.diagnostics.length === 0,
      path: outPath,
      source_hash: finalManifest.source_hash,
      diagnostics: finalManifest.diagnostics,
    });
  }
  const behaviorNavigation = generateBehaviorNavigation({
    project: 'missiond',
    root: process.cwd(),
    repo: process.cwd(),
    target: path.join(outDir, 'compiled-behavior-navigation.json'),
  });
  if (behaviorNavigation.artifact) {
    const outPath = path.join(outDir, 'compiled-behavior-navigation.json');
    fs.writeFileSync(outPath, `${JSON.stringify(behaviorNavigation.artifact, null, 2)}\n`);
    results.push({
      id: 'behavior-navigation',
      ok: (behaviorNavigation.artifact.diagnostics ?? []).length === 0,
      path: outPath,
      source_hash: behaviorNavigation.artifact.source_hash,
      diagnostics: behaviorNavigation.artifact.diagnostics ?? [],
    });
  } else {
    results.push({
      id: 'behavior-navigation',
      ok: false,
      diagnostics: [{ message: 'missiond behavior navigation artifact was not generated' }],
    });
  }
  const ssotRows = results.filter((row) => (
    row.ok && (
      ['v3', 'runtime-config', 'semantic-ir', 'contract-abi', 'universe'].includes(row.id)
      || row.id.startsWith('runtime-domain:')
    )
  ));
  if (ssotRows.length >= 4) {
    const compiledTargets = ssotRows.map((row) => {
      const targetFile = targets.find((target) => target.id === row.id)?.file ?? path.basename(row.path);
      const compiledPath = path.join(outDir, targetFile);
      const compiled = JSON.parse(fs.readFileSync(compiledPath, 'utf8'));
      return {
        id: row.id,
        compiled,
        count: Array.isArray(compiled?.payload?.source_units) ? compiled.payload.source_units.length : 0,
        source_domains: Array.isArray(compiled?.payload?.source_domains) ? compiled.payload.source_domains.length : 0,
      };
    });
    for (const row of compiledTargets) {
      if (row.count === 0) {
        results.push({
          id: `${row.id}-source-units-present`,
          ok: false,
          diagnostics: [{
            message: `${row.id} compiled payload must include non-empty source_units`,
          }],
        });
      }
    }
    const domainDiagnostics = runSemanticRules({
      rules: ['source-domain-hash-consistency'],
      compiledTargets,
    });
    if (domainDiagnostics.length > 0) {
      results.push({
        id: 'source-domain-hash-consistency',
        ok: false,
        diagnostics: domainDiagnostics,
      });
    }
  }
  const semantic = results.find((row) => row.id === 'semantic-ir' && row.ok);
  if (semantic) {
    const semanticPath = path.join(outDir, 'compiled-semantic-ir.json');
    const semanticJson = JSON.parse(fs.readFileSync(semanticPath, 'utf8'));
    const behaviorNavigationJson = readJsonIfExists(path.join(outDir, 'compiled-behavior-navigation.json'))
      ?? readJsonIfExists(path.join(LEGACY_REPO_OUT_DIR, 'compiled-behavior-navigation.json'));
    const slices = buildAgentSlices({ semanticJson, behaviorNavigationJson });
    const slicePath = path.join(outDir, 'compiled-agent-slices.json');
    fs.writeFileSync(slicePath, `${JSON.stringify(slices, null, 2)}\n`);
    results.push({
      id: 'agent-slices',
      ok: (slices.diagnostics ?? []).length === 0,
      path: slicePath,
      source_hash: semanticJson.source_hash,
      diagnostics: slices.diagnostics ?? [],
    });
    const universe = results.find((row) => row.id === 'universe' && row.ok);
    if (universe) {
      const universePath = path.join(outDir, 'compiled-project-universe.json');
      const universeJson = JSON.parse(fs.readFileSync(universePath, 'utf8'));
      const deploymentPolicy = buildDeploymentPolicy(universeJson);
      const deploymentPolicyPath = path.join(outDir, 'compiled-deployment-policy.json');
      fs.writeFileSync(deploymentPolicyPath, `${JSON.stringify(deploymentPolicy, null, 2)}\n`);
      results.push({
        id: 'deployment-policy',
        ok: (deploymentPolicy.diagnostics ?? []).length === 0,
        path: deploymentPolicyPath,
        source_hash: deploymentPolicy.source_hash,
        diagnostics: deploymentPolicy.diagnostics ?? [],
      });
      const projectNavigation = buildProjectAgentNavigation({
        semanticJson,
        universeJson,
        agentSlicesJson: slices,
      });
      const projectNavigationPath = path.join(outDir, 'compiled-project-agent-navigation.json');
      fs.writeFileSync(projectNavigationPath, `${JSON.stringify(projectNavigation, null, 2)}\n`);
      results.push({
        id: 'project-agent-navigation',
        ok: (projectNavigation.diagnostics ?? []).length === 0,
        path: projectNavigationPath,
        source_hash: projectNavigation.source_hash,
        diagnostics: projectNavigation.diagnostics ?? [],
      });
    }
  }
  const workflows = results.find((row) => row.id === 'workflows' && row.ok);
  if (workflows) {
    const workflowsPath = path.join(outDir, 'compiled-workflows.json');
    const workflowsJson = JSON.parse(fs.readFileSync(workflowsPath, 'utf8'));
    const contracts = {
      schema_version: 'missiond.compiled-workflow-contracts.v1',
      source_hash: workflowsJson.source_hash,
      generated_at: null,
      diagnostics: workflowsJson.diagnostics ?? [],
      payload: workflowsJson.payload,
    };
    const contractsPath = path.join(outDir, 'compiled-workflow-contracts.json');
    fs.writeFileSync(contractsPath, `${JSON.stringify(contracts, null, 2)}\n`);
    results.push({ id: 'workflow-contracts', ok: true, path: contractsPath, source_hash: workflowsJson.source_hash });
  }
  const ok = results.every((row) => row.ok);
  const payload = { ok, mode: opts.check ? 'check' : 'write', out_dir: outDir, results };
  if (opts.json) console.log(JSON.stringify(payload, null, 2));
  else if (ok) {
    for (const row of results) console.log(`${row.id}: ${row.path}`);
  } else {
    console.error(JSON.stringify(payload, null, 2));
  }
  process.exit(ok ? 0 : 1);
}

function parseArgs(argv) {
  const opts = { json: false, check: false, write: false, outDir: OUT_DIR };
  for (let i = 0; i < argv.length; i += 1) {
    const arg = argv[i];
    if (arg === '--json') opts.json = true;
    else if (arg === '--check') opts.check = true;
    else if (arg === '--write') opts.write = true;
    else if (arg === '--out-dir') opts.outDir = argv[++i] ?? fail('--out-dir requires a value');
    else if (arg.startsWith('--out-dir=')) opts.outDir = arg.slice('--out-dir='.length);
    else if (arg === '--help' || arg === '-h') {
      console.log('Usage: node scripts/compile-v3-runtime.mjs [--json] [--check|--write] [--out-dir <dir>]');
      process.exit(0);
    } else {
      fail(`unknown argument: ${arg}`);
    }
  }
  if (opts.check && opts.write) fail('--check and --write are mutually exclusive');
  return opts;
}

function fail(message) {
  console.error(message);
  process.exit(2);
}

function readJsonIfExists(file) {
  try {
    return JSON.parse(fs.readFileSync(file, 'utf8'));
  } catch {
    return null;
  }
}

function normalizeSourceUnits(sourceUnits) {
  if (!Array.isArray(sourceUnits)) return '[]';
  return JSON.stringify(sourceUnits.map((unit) => ({
    file: unit?.file ?? null,
    kind: unit?.kind ?? null,
    included_by: unit?.included_by ?? null,
    include_line: unit?.include_line ?? null,
    source_hash: unit?.source_hash ?? null,
  })));
}

function targetCompiledOk(compiled) {
  return compiled
    && typeof compiled === 'object'
    && typeof compiled.schema_version === 'string'
    && typeof compiled.source_hash === 'string'
    && Array.isArray(compiled.diagnostics)
    && compiled.diagnostics.length === 0
    && compiled.payload
    && typeof compiled.payload === 'object';
}

function compiledRuntimeDomainTargets(runtimeConfig) {
  const payload = runtimeConfig?.payload ?? {};
  const sourceUnits = Array.isArray(payload.source_units) ? payload.source_units : [];
  const sourceDomains = Array.isArray(payload.source_domains) ? payload.source_domains : [];
  return RUNTIME_DOMAIN_SPECS.map((spec) => {
    const config = payload[spec.payloadKey];
    if (!config || typeof config !== 'object' || Array.isArray(config)) {
      return {
        id: `runtime-domain:${spec.id}`,
        ok: false,
        domain: spec.id,
        file: spec.file,
        diagnostics: [{
          message: `compiled runtime config missing payload.${spec.payloadKey}`,
        }],
      };
    }
    return {
      id: `runtime-domain:${spec.id}`,
      ok: true,
      domain: spec.id,
      file: spec.file,
      compiled: {
        schema_version: 'missiond.compiled-runtime-domain.v1',
        source_hash: runtimeConfig.source_hash,
        generated_at: null,
        diagnostics: runtimeConfig.diagnostics ?? [],
        payload: {
          domain: spec.id,
          payload_key: spec.payloadKey,
          config,
          runtime_policies: Array.isArray(payload.runtime_policies) ? payload.runtime_policies : [],
          source_units: sourceUnits,
          source_domains: sourceDomains,
        },
      },
    };
  });
}

function compiledFinalConvergenceManifest(contractAbi) {
  const payload = contractAbi?.payload ?? {};
  const facts = Array.isArray(payload.facts) ? payload.facts : [];
  const gate = facts.find((fact) => fact?.kind === 'final_convergence_gate');
  const diagnostics = [];
  if (!gate) diagnostics.push({ message: 'contract ABI missing final_convergence_gate fact' });
  return {
    schema_version: 'missiond.compiled-final-convergence-manifest.v1',
    source_hash: contractAbi.source_hash,
    generated_at: null,
    diagnostics,
    payload: {
      ...(normalizeFinalConvergenceGate(gate) ?? {}),
      source_units: Array.isArray(payload.source_units) ? payload.source_units : [],
      source_domains: Array.isArray(payload.source_domains) ? payload.source_domains : [],
    },
  };
}

function normalizeFinalConvergenceGate(row) {
  if (!row || typeof row !== 'object') return null;
  return {
    id: stringOrNull(row?.id) ?? 'v3-final-convergence',
    liveChecks: normalizeGateChecks(row?.live_checks),
    runtimeChecks: normalizeGateChecks(row?.runtime_checks),
    requiredLiveCheckIds: stringArray(row?.required_live_check_ids ?? row?.requiredLiveCheckIds),
    blueprintNeedles: arrayOrEmpty(row?.blueprint_needles)
      .map((entry) => ({
        id: stringOrNull(entry?.id),
        needle: stringOrNull(entry?.needle),
      }))
      .filter((entry) => entry.id && entry.needle),
    facadeBudgets: arrayOrEmpty(row?.facade_budgets)
      .map((entry) => ({
        id: stringOrNull(entry?.id),
        file: stringOrNull(entry?.file),
        maxLines: positiveIntOrNull(entry?.max_lines ?? entry?.maxLines),
      }))
      .filter((entry) => entry.id && entry.file && entry.maxLines != null),
    requiredSplitFiles: stringArray(row?.required_split_files ?? row?.requiredSplitFiles),
    requiredRuntimeFiles: arrayOrEmpty(row?.required_runtime_files ?? row?.requiredRuntimeFiles)
      .map((entry) => ({
        file: stringOrNull(entry?.file),
        needles: stringArray(entry?.needles),
      }))
      .filter((entry) => entry.file),
    source: row?.source ?? null,
  };
}

function normalizeGateChecks(rows) {
  return arrayOrEmpty(rows)
    .map((entry) => ({
      id: stringOrNull(entry?.id),
      command: stringOrNull(entry?.command),
      argv: stringArray(entry?.argv),
      json: entry?.json === true,
      timeoutMs: positiveIntOrNull(entry?.timeout_ms ?? entry?.timeoutMs) ?? 60_000,
    }))
    .filter((entry) => entry.id && entry.argv.length > 0);
}

function augmentProjectUniverseDomainProxyLifecycle(compiled) {
  const payload = compiled?.payload ?? {};
  const services = Array.isArray(payload.services) ? payload.services : [];
  const metadata = readServiceRuntimeMetadataMap();
  const diagnostics = [];
  for (const service of services) {
    const serviceId = stringOrNull(service?.id);
    if (!serviceId) continue;
    const meta = metadata.get(serviceId) ?? {};
    if (meta.compat_domains?.length) service.compat_domains = meta.compat_domains;
    if (meta.domain_exception_reason) {
      service.domain_exception_reason = meta.domain_exception_reason;
    }
    const proxy = service.proxy ?? meta.proxy ?? null;
    const canonicalDomain = meta.canonical_domain
      ?? normalizeManagedDomain(proxy?.domain)
      ?? firstOwnedDomain(service.domains)
      ?? firstOwnedDomain(domainsFromText([
        service.public_base_url,
        service.api_base_url,
        service.frontend_url,
      ].filter(Boolean).join(' ')));
    if (canonicalDomain) service.canonical_domain = canonicalDomain;
    const health = stringArray(service.health);
    service.health_standard = {
      live: health.includes('/health/live'),
      ready: health.includes('/health/ready'),
      declared_paths: health,
    };
    service.domain_binding_lifecycle = compactObject({
      state_machine: ['planned', 'dns_ready', 'proxy_ready', 'smoke_ready', 'active'],
      failure_state: 'blocked',
      canonical_domain: canonicalDomain,
      compat_domains: meta.compat_domains ?? [],
      active_requires: ['dns_ready', 'proxy_ready', 'smoke_ready'],
      source_ref: `.missiond/v3/shards/universe/service-runtime.lisp#service:${serviceId}`,
    });
    if (proxy && canonicalDomain) {
      service.domain_proxy_intent = compactObject({
        service_id: serviceId,
        project_id: service.project ?? serviceId,
        domain: canonicalDomain,
        upstream: proxy.upstream,
        routes: proxy.routes ?? [],
        kind: proxy.kind ?? 'caddy',
        health,
        smoke_probes: health.filter((path) => path.startsWith('/health')),
        lifecycle: service.domain_binding_lifecycle,
      });
    }
    if (serviceId === 'auth') {
      if (canonicalDomain !== 'auth.xiaojinpro.com') {
        diagnostics.push({
          kind: 'auth_canonical_domain_invalid',
          service_id: serviceId,
          expected: 'auth.xiaojinpro.com',
          actual: canonicalDomain,
        });
      }
      if (stringArray(service.domains).includes('auth.xiaojins.com')) {
        diagnostics.push({
          kind: 'auth_xiaojins_domain_forbidden',
          service_id: serviceId,
          domain: 'auth.xiaojins.com',
        });
      }
      if (!service.domain_exception_reason) {
        diagnostics.push({
          kind: 'auth_domain_exception_reason_missing',
          service_id: serviceId,
        });
      }
    } else if (service.environment === 'production' && proxy?.kind === 'caddy') {
      if (!canonicalDomain) {
        diagnostics.push({ kind: 'canonical_domain_missing', service_id: serviceId });
      }
      if (!proxy.upstream) {
        diagnostics.push({ kind: 'proxy_upstream_missing', service_id: serviceId });
      }
    }
  }
  payload.domain_proxy_diagnostics = diagnostics;
  payload.domain_proxy_summary = {
    services_with_canonical_domain: services.filter((service) => service.canonical_domain).length,
    services_with_proxy_intent: services.filter((service) => service.domain_proxy_intent).length,
    diagnostics: diagnostics.length,
  };
  return compiled;
}

function augmentProjectUniverseDomainManagement(compiled) {
  const payload = compiled?.payload ?? {};
  const config = readDomainControlPlaneConfig();
  const declaredRecords = readDomainRecordMap();
  const rows = new Map();
  const addDomain = (domain, patch = {}) => {
    const normalized = normalizeManagedDomain(domain);
    if (!normalized || domainExcluded(normalized, config)) return;
    const zone = zoneForDomain(normalized, config.managed_zones);
    if (!zone) return;
    const current = rows.get(normalized) ?? {
      domain: normalized,
      zone,
      managed_by: config.authority,
      mutation_policy: config.mutation_policy,
      management_status: 'inventory_only_record_missing',
      sources: [],
    };
    rows.set(normalized, mergeDomainManagementRow(current, patch));
  };

  for (const zone of config.managed_zones) {
    addDomain(zone, {
      source_kind: 'domain-control-managed-zone',
      management_status: 'zone_inventory',
    });
  }
  for (const domain of config.required_domains) {
    addDomain(domain, {
      source_kind: 'domain-control-required-binding',
      management_status: 'required_binding_missing_service_runtime',
    });
  }

  for (const service of arrayOrEmpty(payload.services)) {
    const serviceId = stringOrNull(service?.id);
    const projectId = stringOrNull(service?.project) ?? serviceId;
    for (const domain of stringArray(service?.domains)) {
      addDomain(domain, {
        owner_service_id: serviceId,
        owner_project_id: projectId,
        source_kind: 'service-runtime-domains',
        proxy_intent: service?.domain_proxy_intent,
        smoke_probes: service?.domain_proxy_intent?.smoke_probes,
      });
    }
    for (const domain of domainsFromText([
      service?.public_base_url,
      service?.frontend_url,
      service?.api_base_url,
      service?.frontend_deployment?.production_domain,
    ].filter(Boolean).join(' '))) {
      addDomain(domain, {
        owner_service_id: serviceId,
        owner_project_id: projectId,
        source_kind: 'service-runtime-url',
        proxy_intent: service?.domain_proxy_intent,
        smoke_probes: service?.domain_proxy_intent?.smoke_probes,
      });
    }
  }

  for (const project of arrayOrEmpty(payload.projects)) {
    const projectId = stringOrNull(project?.id);
    const text = [
      ...stringArray(project?.aliases),
      project?.missiond_role,
    ].filter(Boolean).join(' ');
    for (const domain of domainsFromText(text)) {
      addDomain(domain, {
        owner_project_id: projectId,
        source_kind: 'project-registry-domain-reference',
      });
    }
  }

  for (const [domain, records] of declaredRecords) {
    addDomain(domain, {
      source_kind: 'dns-records',
      declared_records: records,
      management_status: records.some((record) => record.authority === config.authority)
        ? 'desired_state_declared'
        : 'legacy_cloudflare_record_to_import',
    });
  }

  const domains = [...rows.values()]
    .map((row) => compactObject({
      ...row,
      sources: uniqueStrings(row.sources),
      declared_records: arrayOrEmpty(row.declared_records),
    }))
    .sort((a, b) => String(a.zone).localeCompare(String(b.zone)) || String(a.domain).localeCompare(String(b.domain)));
  payload.domain_management = {
    schema_version: config.schema,
    authority: config.authority,
    entrypoint: config.entrypoint,
    managed_zones: config.managed_zones,
    excluded_domains: config.excluded_domains,
    required_domains: config.required_domains,
    mutation_policy: config.mutation_policy,
    default_mode: config.default_mode,
    source_kinds: config.source_kinds,
    agent_prompt: config.agent_prompt,
    domains,
    summary: {
      domain_count: domains.length,
      zone_count: config.managed_zones.length,
      required_domain_count: config.required_domains.length,
      desired_state_declared_count: domains.filter((row) => row.management_status === 'desired_state_declared').length,
      legacy_import_count: domains.filter((row) => row.management_status === 'legacy_cloudflare_record_to_import').length,
      inventory_only_count: domains.filter((row) => row.management_status?.includes('inventory')).length,
    },
    source_ref: '.missiond/v3/shards/universe/service-runtime.lisp#domain-control-plane',
  };
  return compiled;
}

function mergeDomainManagementRow(current, patch) {
  const next = {
    ...current,
    ...compactObject({
      owner_project_id: patch.owner_project_id ?? current.owner_project_id,
      owner_service_id: patch.owner_service_id ?? current.owner_service_id,
      management_status: preferManagementStatus(current.management_status, patch.management_status),
    }),
  };
  const source = compactObject({
    kind: patch.source_kind,
    owner_project_id: patch.owner_project_id,
    owner_service_id: patch.owner_service_id,
  });
  if (source.kind) next.sources = [...arrayOrEmpty(current.sources), source];
  if (patch.declared_records) {
    next.declared_records = [
      ...arrayOrEmpty(current.declared_records),
      ...arrayOrEmpty(patch.declared_records),
    ];
  }
  if (patch.proxy_intent && !next.proxy_intent) next.proxy_intent = patch.proxy_intent;
  if (patch.smoke_probes && !next.smoke_probes) next.smoke_probes = patch.smoke_probes;
  return next;
}

function preferManagementStatus(current, candidate) {
  if (!candidate) return current;
  const rank = {
    desired_state_declared: 5,
    legacy_cloudflare_record_to_import: 4,
    required_binding_missing_service_runtime: 3,
    zone_inventory: 2,
    inventory_only_record_missing: 1,
  };
  return (rank[candidate] ?? 0) >= (rank[current] ?? 0) ? candidate : current;
}

function readDomainControlPlaneConfig() {
  const defaults = {
    schema: 'missiond.domain-control-plane.v1',
    authority: 'xjp-domain-service',
    entrypoint: 'https://domains.xiaojins.com/v1/domains',
    managed_zones: ['xiaojins.com', 'xiaojinpro.top', 'xiaojinpro.com'],
    required_domains: [],
    excluded_domains: ['xjp-asr-web.vercel.app', 'cname.vercel-dns.com'],
    mutation_policy: 'approval-required',
    default_mode: 'read-only-inventory',
    source_kinds: [],
    agent_prompt: 'For domain or DNS questions, consult xjp-domain-service before answering authority or mutating DNS.',
  };
  const text = safeRead('.missiond/v3/shards/universe/service-runtime.lisp');
  const start = text.indexOf('(domain-control-plane');
  if (start === -1) return defaults;
  const close = findBalancedClose(text, start);
  const form = close === -1 ? '' : text.slice(start, close + 1);
  return {
    schema: keywordValue(form, 'schema') ?? defaults.schema,
    authority: keywordValue(form, 'authority') ?? defaults.authority,
    entrypoint: keywordValue(form, 'entrypoint') ?? defaults.entrypoint,
    managed_zones: keywordListValue(form, 'managed-zones').map(normalizeManagedDomain).filter(Boolean),
    required_domains: keywordListValue(form, 'required-domains').map(normalizeManagedDomain).filter(Boolean),
    excluded_domains: keywordListValue(form, 'excluded-domains').map(normalizeManagedDomain).filter(Boolean),
    mutation_policy: keywordValue(form, 'mutation-policy') ?? defaults.mutation_policy,
    default_mode: keywordValue(form, 'default-mode') ?? defaults.default_mode,
    source_kinds: keywordListValue(form, 'source-kinds'),
    agent_prompt: keywordValue(form, 'agent-prompt') ?? defaults.agent_prompt,
  };
}

function readDomainRecordMap() {
  const records = new Map();
  const text = safeRead('.missiond/v3/shards/universe/service-runtime.lisp');
  for (const { serviceId, body } of extractServiceRuntimeForms(text)) {
    for (const record of extractDnsRecordForms(body).map((form) => normalizeDnsRecordForm(form, serviceId))) {
      if (!record?.name) continue;
      const domain = normalizeManagedDomain(record.name);
      if (!domain) continue;
      if (!records.has(domain)) records.set(domain, []);
      records.get(domain).push(record);
    }
  }
  return records;
}

function extractDnsRecordForms(serviceForm) {
  const forms = [];
  const single = parseKeywordForm(serviceForm, 'dns-record');
  if (single) forms.push(single);
  const plural = parseKeywordBracketBody(serviceForm, 'dns-records');
  if (plural) forms.push(...extractColonForms(plural));
  return forms;
}

function extractColonForms(text) {
  const forms = [];
  let cursor = 0;
  while (cursor < text.length) {
    const start = text.indexOf('(:', cursor);
    if (start === -1) break;
    const close = findBalancedClose(text, start);
    if (close === -1) break;
    forms.push(text.slice(start + 1, close));
    cursor = close + 1;
  }
  return forms;
}

function parseKeywordBracketBody(text, key) {
  const start = keywordMarkerIndex(text, key);
  if (start === -1) return null;
  const open = text.indexOf('[', start + key.length + 1);
  if (open === -1) return null;
  let depth = 0;
  let inString = false;
  let escaped = false;
  for (let i = open; i < text.length; i += 1) {
    const ch = text[i];
    if (escaped) {
      escaped = false;
      continue;
    }
    if (ch === '\\') {
      escaped = true;
      continue;
    }
    if (ch === '"') {
      inString = !inString;
      continue;
    }
    if (inString) continue;
    if (ch === '[') depth += 1;
    if (ch === ']') {
      depth -= 1;
      if (depth === 0) return text.slice(open + 1, i);
    }
  }
  return null;
}

function normalizeDnsRecordForm(form, serviceId) {
  return compactObject({
    service_id: serviceId,
    type: keywordValue(form, 'type'),
    name: keywordValue(form, 'name'),
    content: keywordValue(form, 'content'),
    proxied: boolOrNull(keywordValue(form, 'proxied')),
    ttl: numberOrNull(keywordValue(form, 'ttl')),
    authority: keywordValue(form, 'authority'),
    status: keywordValue(form, 'status') ?? 'declared',
  });
}

function normalizeManagedDomain(value) {
  const raw = stringOrNull(value);
  if (!raw) return null;
  const withoutScheme = raw.replace(/^https?:\/\//i, '');
  const host = withoutScheme.split('/')[0].split('?')[0].split('#')[0].trim().toLowerCase();
  return /^[a-z0-9-]+(\.[a-z0-9-]+)+$/.test(host) ? host : null;
}

function domainsFromText(text) {
  const matches = String(text ?? '').match(/\b(?:[a-z0-9-]+\.)+(?:com|top|ai|cn|app|io)\b/gi) ?? [];
  return [...new Set(matches.map(normalizeManagedDomain).filter(Boolean))];
}

function domainExcluded(domain, config) {
  if (config.excluded_domains.includes(domain)) return true;
  if (domain.endsWith('.vercel.app')) return true;
  if (domain === 'ghcr.io' || domain.endsWith('.ghcr.io')) return true;
  return false;
}

function zoneForDomain(domain, zones) {
  return zones
    .filter((zone) => domain === zone || domain.endsWith(`.${zone}`))
    .sort((a, b) => b.length - a.length)[0] ?? null;
}

function firstOwnedDomain(domains) {
  return stringArray(domains)
    .map(normalizeManagedDomain)
    .find((domain) => domain?.endsWith('.xiaojins.com') || domain === 'xiaojins.com')
    ?? stringArray(domains).map(normalizeManagedDomain).find(Boolean)
    ?? null;
}

function keywordListValue(text, key) {
  const escaped = key.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
  const match = text.match(new RegExp(`:${escaped}\\s+\\[([^\\]]*)\\]`));
  if (!match) return [];
  const values = [];
  const tokenRe = /"([^"]+)"|([^\s\]]+)/g;
  for (const token of match[1].matchAll(tokenRe)) values.push(token[1] ?? token[2]);
  return values.filter((value) => value && value !== 'nil');
}

function numberOrNull(value) {
  const number = Number(value);
  return Number.isFinite(number) ? number : null;
}

function uniqueStrings(values) {
  const seen = new Set();
  const result = [];
  for (const value of values) {
    const key = JSON.stringify(value);
    if (seen.has(key)) continue;
    seen.add(key);
    result.push(value);
  }
  return result;
}

function augmentProjectUniverseDeploymentChannels(compiled) {
  const serviceChannels = readServiceDeploymentChannelMap();
  const inference = inferDeploymentChannelsForUniverse(compiled, serviceChannels);
  const services = Array.isArray(compiled?.payload?.services) ? compiled.payload.services : [];
  const diagnostics = [...inference.diagnostics];
  const allChannels = [];
  for (const service of services) {
    const serviceId = stringOrNull(service?.id);
    if (!serviceId) continue;
    const channels = serviceChannels.get(serviceId);
    if (channels?.deployment) service.deployment = channels.deployment;
    if (channels?.frontend_deployment) service.frontend_deployment = channels.frontend_deployment;
    if (channels?.build_lane) service.build_lane = channels.build_lane;
    if (channels?.proxy) service.proxy = channels.proxy;
    if (channels?.jarvis_runtime_topology) service.jarvis_runtime_topology = channels.jarvis_runtime_topology;

    const projectId = stringOrNull(service?.project) ?? serviceId;
    const explicit = channels?.deployment_channels ?? [];
    const legacy = channels ? buildServiceDeploymentChannels(service, channels) : [];
    const inferred = inference.channelsByService.get(serviceId) ?? [];
    const merged = mergeDeploymentChannels([...explicit, ...legacy, ...inferred], {
      serviceId,
      projectId,
    });
    service.deployment_channels = merged;
    allChannels.push(...merged);

    const serviceDiagnostics = deploymentChannelDiagnosticsForService(service, merged);
    diagnostics.push(...serviceDiagnostics);
  }
  compiled.payload.deployment_channels = allChannels;
  compiled.payload.deployment_channel_diagnostics = diagnostics;
  compiled.payload.deployment_channel_summary = buildDeploymentChannelSummary(allChannels, diagnostics);
  compiled.payload.jarvis_runtime_topologies = services
    .map((service) => service.jarvis_runtime_topology)
    .filter((topology) => topology && typeof topology === 'object');
  return compiled;
}

function buildServiceDeploymentChannels(service, channels) {
  const serviceId = stringOrNull(service?.id);
  const projectId = stringOrNull(service?.project) ?? serviceId;
  const rows = [];
  if (channels.build_lane) {
    rows.push(compactObject({
      id: `${serviceId}:build`,
      service_id: serviceId,
      project_id: projectId,
      surface: 'build',
      substrate: channels.build_lane.id ?? 'privatecloud',
      channel_kind: nativeBuildChannelKind(channels.build_lane),
      runner_role: 'build_runner',
      build_lane: channels.build_lane.id,
      builder: channels.build_lane.builder,
      executor: channels.build_lane.executor,
      source_sync: channels.build_lane.source_sync,
      dockerfile: channels.build_lane.dockerfile,
      manifest: channels.build_lane.manifest,
      artifact_lane: channels.build_lane.artifact_lane,
      image: channels.build_lane.image,
      authority: channels.build_lane.authority ?? 'deploy-center',
      target_side_build_prohibited: channels.build_lane.target_side_build_prohibited,
      declared_status: 'declared',
      observed_status: 'not_queried',
      drift_status: 'not_checked',
      source_ref: `.missiond/v3/shards/universe/service-runtime.lisp#service:${serviceId}:build-lane`,
    }));
  }
  if (channels.deployment) {
    rows.push(compactObject({
      id: `${serviceId}:runtime`,
      service_id: serviceId,
      project_id: projectId,
      surface: 'runtime',
      substrate: channels.deployment.substrate,
      channel_kind: runtimeChannelKind(channels.deployment.substrate),
      runner_role: 'runtime_runner',
      deploy_center_slug: channels.deployment.dc_slug,
      runtime_target: channels.deployment.runtime_target,
      executor: channels.deployment.executor,
      container: channels.deployment.container,
      host_bind: channels.deployment.host_bind ?? channels.deployment.local_bind,
      proxy: channels.deployment.proxy,
      artifact_lane: channels.deployment.artifact_delivery_lane,
      image_env: channels.deployment.image_env,
      authority: channels.deployment.authority,
      target_side_build_prohibited: channels.deployment.target_side_build_prohibited,
      declared_status: 'declared',
      observed_status: 'not_queried',
      drift_status: 'not_checked',
      source_ref: `.missiond/v3/shards/universe/service-runtime.lisp#service:${serviceId}:deployment`,
    }));
  }
  if (channels.frontend_deployment) {
    rows.push(compactObject({
      id: `${serviceId}:frontend`,
      service_id: serviceId,
      project_id: projectId,
      surface: 'frontend',
      substrate: channels.frontend_deployment.substrate,
      channel_kind: channels.frontend_deployment.substrate === 'vercel' ? 'vercel' : 'unknown',
      runner_role: 'frontend_runner',
      project: channels.frontend_deployment.project,
      root_directory: channels.frontend_deployment.root_directory,
      production_domain: channels.frontend_deployment.production_domain,
      fallback_domain: channels.frontend_deployment.fallback_domain,
      authority: channels.frontend_deployment.authority ?? 'vercel',
      declared_status: 'declared',
      observed_status: 'not_queried',
      drift_status: 'not_checked',
      source_ref: `.missiond/v3/shards/universe/service-runtime.lisp#service:${serviceId}:frontend-deployment`,
    }));
  }
  return rows;
}

function inferDeploymentChannelsForUniverse(compiled, serviceChannels) {
  const channelsByService = new Map();
  const diagnostics = [];
  const services = Array.isArray(compiled?.payload?.services) ? compiled.payload.services : [];
  for (const service of services) {
    const serviceId = stringOrNull(service?.id);
    if (!serviceId) continue;
    const declared = serviceChannels.get(serviceId) ?? {};
    const root = stringOrNull(service?.root);
    const projectId = stringOrNull(service?.project) ?? serviceId;
    const deployCenterSlug = declared.deployment?.dc_slug
      ?? stringOrNull(service?.deployment?.dc_slug ?? service?.deployment?.dcSlug)
      ?? stringOrNull(service?.support_catalog?.deploy_center_slug ?? service?.supportCatalog?.deployCenterSlug);
    const needsBuildChannel = serviceNeedsBuildChannel(service, declared);
    const inferred = [];

    if (root) {
      inferred.push(...inferProjectLocalDeployCenterChannels({ serviceId, projectId, root }));
      if (needsBuildChannel) {
        const workflowFacts = readGithubWorkflowFactsForRoot(root);
        const serviceRegistry = readServiceRegistryForRoot(root);
        const registryEntry = matchServiceRegistryEntry(serviceId, deployCenterSlug, serviceRegistry);
        const workflow = selectWorkflowFact({ serviceId, deployCenterSlug, registryEntry, workflowFacts });
        if (workflow) {
          inferred.push(githubActionsBuildChannel({ serviceId, projectId, workflow, registryEntry }));
        }
      }
    }

    if (inferred.length > 0) {
      channelsByService.set(serviceId, inferred);
    } else if (root && needsBuildChannel) {
      diagnostics.push({
        kind: 'build_channel_not_inferred',
        service_id: serviceId,
        project_id: projectId,
        root,
        message: `${serviceId} has runtime/backend facts but no declared or inferred build channel`,
      });
    }
  }
  return { channelsByService, diagnostics };
}

function inferProjectLocalDeployCenterChannels({ serviceId, projectId, root }) {
  const file = firstExistingPath([
    path.join(root, `deploy/deploy-center/project.${serviceId}.json`),
    path.join(root, 'deploy/deploy-center/project.json'),
  ]);
  if (!file) return [];
  const config = readJsonIfExists(file);
  if (!config || typeof config !== 'object') return [];
  const project = config.project ?? {};
  const stages = config.stages ?? {};
  const rows = [];
  const build = stages.build;
  const buildDeployType = stringOrNull(build?.config?.deploy_type);
  if (build?.enabled !== false && (buildDeployType === 'native_workflow' || buildDeployType === 'docker_build')) {
    const isNativeWorkflow = buildDeployType === 'native_workflow';
    rows.push(compactObject({
      id: `${serviceId}:build:project-local`,
      service_id: serviceId,
      project_id: projectId,
      surface: 'build',
      substrate: isNativeWorkflow ? 'privatecloud-rust-build-lane' : 'privatecloud-docker-build-lane',
      channel_kind: isNativeWorkflow ? 'native_workflow' : 'privatecloud_docker_build',
      runner_role: 'build_runner',
      authority: 'deploy-center',
      source_ref: `${file}#stages.build`,
      deploy_center_slug: stringOrNull(project.slug) ?? stringOrNull(build.stage_project_slug),
      executor: stringOrNull(build.executor_name),
      builder: stringOrNull(build.config.builder_id),
      source_sync: 'deploy-center-codebase',
      dockerfile: stringOrNull(build.config.dockerfile),
      image: stringOrNull(build.config.image),
      artifact_lane: stringOrNull(build.config.artifact_lane),
      workflow: buildDeployType,
      target_side_build_prohibited: true,
      declared_status: 'inferred',
      observed_status: 'not_queried',
      drift_status: 'not_checked',
    }));
  }
  const deploy = stages.deploy;
  if (deploy?.enabled !== false && deploy?.config) {
    rows.push(compactObject({
      id: `${serviceId}:runtime:project-local`,
      service_id: serviceId,
      project_id: projectId,
      surface: 'runtime',
      substrate: 'deploy-center',
      channel_kind: 'deploy_center_runtime',
      runner_role: 'runtime_runner',
      authority: 'deploy-center',
      source_ref: `${file}#stages.deploy`,
      deploy_center_slug: stringOrNull(project.slug) ?? stringOrNull(deploy.stage_project_slug),
      executor: stringOrNull(deploy.executor_name),
      image: stringOrNull(deploy.config.image),
      image_env: stringOrNull(deploy.config.image_env),
      target_side_build_prohibited: deploy.config.no_build === true,
      declared_status: 'inferred',
      observed_status: 'not_queried',
      drift_status: 'not_checked',
    }));
  }
  return rows;
}

function firstExistingPath(paths) {
  for (const candidate of paths) {
    if (fs.existsSync(candidate)) return candidate;
  }
  return null;
}

function githubActionsBuildChannel({ serviceId, projectId, workflow, registryEntry }) {
  const image = stringOrNull(workflow.image) ?? stringOrNull(registryEntry?.image);
  const deployCenterSlug = stringOrNull(workflow.dc_project) ?? stringOrNull(registryEntry?.dc_slug);
  return compactObject({
    id: `${serviceId}:build:github-actions`,
    service_id: serviceId,
    project_id: projectId,
    surface: 'build',
    substrate: 'github-actions',
    channel_kind: 'github_actions',
    runner_role: 'build_runner',
    authority: 'github-actions',
    source_ref: workflow.source_ref,
    workflow: workflow.file,
    deploy_center_slug: deployCenterSlug,
    image,
    dockerfile: stringOrNull(workflow.dockerfile),
    service_name: stringOrNull(workflow.service_name),
    declared_status: 'inferred',
    observed_status: 'not_queried',
    drift_status: 'not_checked',
  });
}

function mergeDeploymentChannels(channels, defaults = {}) {
  const rows = [];
  const seen = new Set();
  for (const channel of channels) {
    if (!channel || typeof channel !== 'object') continue;
    const serviceId = stringOrNull(channel.service_id ?? channel.serviceId)
      ?? stringOrNull(defaults.serviceId)
      ?? 'service';
    const projectId = stringOrNull(channel.project_id ?? channel.projectId)
      ?? stringOrNull(channel.project)
      ?? stringOrNull(defaults.projectId);
    const surface = stringOrNull(channel.surface) ?? 'runtime';
    const key = `${serviceId}:${surface}`;
    if (seen.has(key)) continue;
    seen.add(key);
    rows.push(compactObject({
      ...channel,
      service_id: serviceId,
      project_id: projectId,
      surface,
      channel_kind: stringOrNull(channel.channel_kind ?? channel.channelKind) ?? channelKindForChannel(channel),
      runner_role: stringOrNull(channel.runner_role ?? channel.runnerRole) ?? defaultRunnerRoleForSurface(surface),
      declared_status: stringOrNull(channel.declared_status ?? channel.declaredStatus) ?? 'declared',
      observed_status: stringOrNull(channel.observed_status ?? channel.observedStatus) ?? 'not_queried',
      drift_status: stringOrNull(channel.drift_status ?? channel.driftStatus) ?? 'not_checked',
    }));
  }
  return rows;
}

function defaultRunnerRoleForSurface(surface) {
  if (surface === 'build') return 'build_runner';
  if (surface === 'runtime') return 'runtime_runner';
  if (surface === 'frontend') return 'frontend_runner';
  if (surface === 'domain') return 'domain_runner';
  if (surface === 'self_update') return 'self_update_runner';
  return null;
}

function deploymentChannelDiagnosticsForService(service, channels) {
  const serviceId = stringOrNull(service?.id);
  if (!serviceId) return [];
  const diagnostics = [];
  for (const channel of channels) {
    if (!stringOrNull(channel?.service_id ?? channel?.serviceId)) {
      diagnostics.push({
        kind: 'deployment_channel_missing_service_id',
        service_id: serviceId,
        project_id: stringOrNull(service?.project) ?? serviceId,
        channel_id: stringOrNull(channel?.id),
        surface: stringOrNull(channel?.surface),
        message: `${serviceId} has a deployment channel without service_id`,
      });
    }
    if (!stringOrNull(channel?.project_id ?? channel?.projectId)) {
      diagnostics.push({
        kind: 'deployment_channel_missing_project_id',
        service_id: serviceId,
        project_id: stringOrNull(service?.project) ?? serviceId,
        channel_id: stringOrNull(channel?.id),
        surface: stringOrNull(channel?.surface),
        message: `${serviceId} has a deployment channel without project_id`,
      });
    }
  }
  if (!serviceNeedsBuildChannel(service, {})) return diagnostics;
  const buildChannels = channels.filter((channel) => channel.surface === 'build');
  if (buildChannels.length === 1) return diagnostics;
  diagnostics.push({
    kind: buildChannels.length === 0 ? 'missing_build_channel' : 'multiple_build_channels',
    service_id: serviceId,
    project_id: stringOrNull(service?.project) ?? serviceId,
    build_channel_count: buildChannels.length,
    message: `${serviceId} must expose exactly one build channel or an explicit exception`,
  });
  return diagnostics;
}

function buildDeploymentChannelSummary(channels, diagnostics) {
  const byKind = {};
  const bySurface = {};
  for (const channel of channels) {
    const kind = stringOrNull(channel.channel_kind) ?? 'unknown';
    const surface = stringOrNull(channel.surface) ?? 'unknown';
    byKind[kind] = (byKind[kind] ?? 0) + 1;
    bySurface[surface] = (bySurface[surface] ?? 0) + 1;
  }
  return {
    total: channels.length,
    by_kind: byKind,
    by_surface: bySurface,
    diagnostics: diagnostics.length,
  };
}

function channelKindForChannel(channel) {
  if (channel.surface === 'build') return nativeBuildChannelKind(channel);
  if (channel.surface === 'runtime') return runtimeChannelKind(channel.substrate);
  if (channel.surface === 'frontend' && channel.substrate === 'vercel') return 'vercel';
  return 'unknown';
}

function nativeBuildChannelKind(buildLane) {
  const id = stringOrNull(buildLane?.id ?? buildLane?.build_lane ?? buildLane?.buildLane ?? buildLane?.substrate);
  if (id?.includes('docker-build') || id?.includes('docker_build')) return 'privatecloud_docker_build';
  if (id?.includes('privatecloud') || id?.includes('native')) return 'native_workflow';
  return 'unknown';
}

function runtimeChannelKind(substrate) {
  if (substrate === 'deploy-center') return 'deploy_center_runtime';
  if (substrate === 'gcp-vm') return 'gcp_vm';
  if (substrate === 'vercel') return 'vercel';
  if (substrate === 'kubernetes') return 'kubernetes';
  if (substrate === 'local-node' || substrate === 'launchd') return 'local_runtime';
  return substrate ? String(substrate).replaceAll('-', '_') : 'unknown';
}

function serviceNeedsBuildChannel(service, declared) {
  if (declared?.build_lane || (declared?.deployment_channels ?? []).some((channel) => channel.surface === 'build')) {
    return false;
  }
  const environment = stringOrNull(service?.environment) ?? '';
  if (environment === 'local-dev') return false;
  const deployment = declared?.deployment ?? service?.deployment ?? {};
  const substrate = stringOrNull(deployment?.substrate);
  if (!substrate) return Boolean(service?.backend);
  if (['vercel', 'lovable-or-static-host', 'local-node', 'gcp-caddy-edge'].includes(substrate)) return false;
  return Boolean(service?.backend) || ['deploy-center', 'gcp-vm', 'aliyun-ecs', 'kubernetes'].includes(substrate);
}

function compactObject(value) {
  return Object.fromEntries(Object.entries(value).filter(([, item]) => item !== null && item !== undefined && item !== ''));
}

function buildDeploymentPolicy(universeJson) {
  const payload = universeJson?.payload ?? {};
  const projects = Array.isArray(payload.projects) ? payload.projects : [];
  const services = Array.isArray(payload.services) ? payload.services : [];
  const projectById = new Map(projects.map((project) => [project?.id, project]));
  const maturityById = readProjectMaturityMap();
  const deploymentByService = readServiceDeploymentMap();
  const rows = [];

  for (const service of services) {
    const serviceId = stringOrNull(service?.id);
    if (!serviceId) continue;
    const projectId = stringOrNull(service?.project) ?? serviceId;
    const project = projectById.get(projectId) ?? projectById.get(serviceId) ?? {};
    const maturity = maturityById.get(projectId) ?? maturityById.get(serviceId) ?? 'unknown';
    const environment = stringOrNull(service?.environment) ?? 'unknown';
    const strict = environment === 'production' || maturity === 'M5' || maturity === 'M6';
    const deployment = deploymentByService.get(serviceId) ?? {};
    const supportCatalog = service?.support_catalog ?? service?.supportCatalog ?? {};
    const substrate = stringOrNull(deployment.substrate);
    const runtimeTarget = stringOrNull(
      deployment.runtime_target
      ?? deployment.runtimeTarget
      ?? supportCatalog.runtime_target
      ?? supportCatalog.runtimeTarget,
    );
    const artifactLane = deployment.artifact_delivery_lane
      ?? deployment.artifactDeliveryLane
      ?? deployment.artifact_lane
      ?? deployment.artifactLane
      ?? defaultArtifactLaneForDeployment(deployment, strict);
    const targetSideBuildAllowed = deployment.target_side_build_allowed
      ?? deployment.targetSideBuildAllowed
      ?? defaultTargetSideBuildAllowed(deployment, strict);
    rows.push({
      project_id: projectId,
      service_id: serviceId,
      deploy_center_slug: deployment.dc_slug ?? supportCatalog.deploy_center_slug ?? (deployment.substrate === 'deploy-center' ? serviceId : null),
      runtime_target: runtimeTarget,
      domains: stringArray(service?.domains),
      aliases: stringArray(project?.aliases),
      maturity,
      environment,
      manifest_required: strict,
      immutable_image_required: strict,
      runtime_digest_required: strict,
      smoke_required: strict,
      db_adoption_required: serviceId.includes('payments') || projectId.includes('payments'),
      release_lease_required: strict,
      artifact_lane: artifactLane,
      target_side_build_allowed: targetSideBuildAllowed,
      approval_policy: strict ? 'deploy-center-policy-or-explicit-board-approval' : 'project-policy',
      runtime_fact_authority: 'deploy-center',
      closure_authority: 'deploy-center-release-evidence',
      diagnostic_profiles: strict
        ? ['deploy_provenance_snapshot', 'container_inventory', 'dependency_manifest_scan', 'supply_chain_ioc_scan']
        : ['deploy_provenance_snapshot'],
      closure_required_fields: strict
        ? ['ReleasePlan', 'RunnerBinding', 'SecretRequirement', 'ReleaseLease', 'RuntimeObservation', 'ReleaseEvidence', 'ClosureVerdict']
        : ['ReleasePlan', 'ReleaseEvidence', 'ClosureVerdict'],
      fail_closed_blockers: strict
        ? failClosedBlockersFor({ serviceId, projectId, deployment, supportCatalog, runtimeTarget, substrate })
        : [],
    });
  }

  return {
    schema_version: 'missiond.compiled-deployment-policy.v1',
    source_hash: universeJson.source_hash,
    generated_at: null,
    diagnostics: [],
    payload: {
      authority: 'missiond-v3-ssot',
      runtime_fact_authority: 'deploy-center',
      gate_defaults: {
        prod_or_m5_m6: {
          manifest_required: true,
          immutable_image_required: true,
          runtime_digest_required: true,
          smoke_required: true,
          release_lease_required: true,
          target_side_build_allowed: false,
          approval_policy: 'deploy-center-policy-or-explicit-board-approval',
          diagnostic_profiles: ['deploy_provenance_snapshot', 'container_inventory', 'dependency_manifest_scan', 'supply_chain_ioc_scan'],
        },
      },
      closure_state_machine: [
        'classify_change',
        'preflight',
        'build_candidate',
        'acquire_release_lease',
        'deploy',
        'runtime_observe',
        'deep_smoke',
        'closure_verdict',
        'release_or_rollback',
      ],
      closure_verdicts: ['success', 'failed', 'blocked', 'stale', 'provenance_partial'],
      typed_diagnostics: [
        'reported_digest_missing',
        'runtime_digest_mismatch',
        'provenance_partial',
        'db_adoption_required',
        'abi_freshness_mismatch',
        'release_lease_conflict',
        'deployment_lane_mismatch',
        'deploy_blocked_by_secret_store',
        'release_plan_missing',
        'release_plan_blocked',
        'build_runner_unavailable',
        'gcp_build_forbidden',
        'target_side_build_forbidden',
        'macmini_lane_forbidden',
        'runner_required_env_missing',
        'secret_availability_missing',
      ],
      policies: rows,
      source_units: Array.isArray(payload.source_units) ? payload.source_units : [],
      source_domains: Array.isArray(payload.source_domains) ? payload.source_domains : [],
    },
  };
}

function defaultArtifactLaneForDeployment(deployment, strict) {
  const substrate = stringOrNull(deployment?.substrate);
  const runtimeTarget = stringOrNull(deployment?.runtime_target ?? deployment?.runtimeTarget);
  if (deployment?.artifact_lanes && Array.isArray(deployment.artifact_lanes) && deployment.artifact_lanes.length > 0) {
    return deployment.artifact_lanes[0];
  }
  if (deployment?.artifact_lane || deployment?.artifactLane) return deployment.artifact_lane ?? deployment.artifactLane;
  if (substrate === 'deploy-center') return 'cloud-registry-lane';
  if (substrate === 'vercel') return 'vercel-build-lane';
  if (runtimeTarget === 'ecs-pcea') return 'cn-oss-bundle-lane';
  if (runtimeTarget === 'gcp-runtime') return 'cloud-registry-lane';
  return strict ? 'deploy-center-required-lane' : 'project-defined-lane';
}

function defaultTargetSideBuildAllowed(deployment, strict) {
  const lane = defaultArtifactLaneForDeployment(deployment, strict);
  if (lane === 'macmini-codebase-local-build-lane') return true;
  if (lane === 'manual-break-glass-lane') return false;
  return !strict;
}

function failClosedBlockersFor({ serviceId, projectId, deployment, supportCatalog, runtimeTarget, substrate }) {
  const blockers = [];
  const deployCenterSlug = deployment?.dc_slug ?? supportCatalog?.deploy_center_slug ?? (substrate === 'deploy-center' ? serviceId : null);
  if (substrate === 'deploy-center' && !deployCenterSlug) blockers.push('deploy_center_slug_missing');
  if ((substrate === 'deploy-center' || substrate === 'gcp-vm' || substrate === 'aliyun-ecs') && !runtimeTarget) {
    blockers.push('runtime_target_missing');
  }
  if (serviceId.includes('payments') || projectId.includes('payments')) blockers.push('db_adoption_plan_required');
  return blockers;
}

function readProjectMaturityMap() {
  const file = '.missiond/v3/shards/universe/project-maturity.lisp';
  const map = new Map();
  let text = '';
  try {
    text = fs.readFileSync(file, 'utf8');
  } catch {
    return map;
  }
  const re = /\(maturity\s+:id\s+([^\s\)]+)[\s\S]*?:current\s+([^\s\)]+)/g;
  for (const match of text.matchAll(re)) {
    map.set(match[1], match[2]);
  }
  return map;
}

function readServiceDeploymentMap() {
  const map = new Map();
  for (const [serviceId, channels] of readServiceDeploymentChannelMap()) {
    if (channels.deployment) map.set(serviceId, channels.deployment);
  }
  return map;
}

function readServiceDeploymentChannelMap() {
  const file = '.missiond/v3/shards/universe/service-runtime.lisp';
  const map = new Map();
  let text = '';
  try {
    text = fs.readFileSync(file, 'utf8');
  } catch {
    return map;
  }
  for (const { serviceId, body } of extractServiceRuntimeForms(text)) {
    const projectId = keywordValue(body, 'project') ?? serviceId;
    const deployment = parseKeywordForm(body, 'deployment');
    const frontendDeployment = parseKeywordForm(body, 'frontend-deployment');
    const buildLane = parseKeywordForm(body, 'build-lane');
    const proxy = parseKeywordForm(body, 'proxy');
    const jarvisRuntimeTopology = parseKeywordForm(body, 'jarvis-runtime-topology');
    const deploymentChannels = parseKeywordForm(body, 'deployment-channels');
    if (!deployment && !frontendDeployment && !buildLane && !proxy && !jarvisRuntimeTopology && !deploymentChannels) continue;
    map.set(serviceId, compactObject({
      deployment: deployment ? normalizeDeploymentForm(deployment) : null,
      frontend_deployment: frontendDeployment ? normalizeFrontendDeploymentForm(frontendDeployment) : null,
      build_lane: buildLane ? normalizeBuildLaneForm(buildLane) : null,
      proxy: proxy ? normalizeProxyForm(proxy) : null,
      jarvis_runtime_topology: jarvisRuntimeTopology
        ? normalizeJarvisRuntimeTopologyForm(jarvisRuntimeTopology, serviceId)
        : null,
      deployment_channels: deploymentChannels
        ? normalizeExplicitDeploymentChannelsForm(deploymentChannels, serviceId, projectId)
        : [],
    }));
  }
  return map;
}

function readServiceRuntimeMetadataMap() {
  const file = '.missiond/v3/shards/universe/service-runtime.lisp';
  const map = new Map();
  let text = '';
  try {
    text = fs.readFileSync(file, 'utf8');
  } catch {
    return map;
  }
  for (const { serviceId, body } of extractServiceRuntimeForms(text)) {
    const proxy = parseKeywordForm(body, 'proxy');
    map.set(serviceId, compactObject({
      canonical_domain: normalizeManagedDomain(
        keywordValue(body, 'canonical-domain')
          ?? keywordValue(body, 'canonical_domain'),
      ),
      compat_domains: keywordListValue(body, 'compat-domains')
        .concat(keywordListValue(body, 'compat_domains'))
        .map(normalizeManagedDomain)
        .filter(Boolean),
      domain_exception_reason: keywordValue(body, 'domain-exception-reason')
        ?? keywordValue(body, 'domain_exception_reason'),
      proxy: proxy ? normalizeProxyForm(proxy) : null,
    }));
  }
  return map;
}

function extractServiceRuntimeForms(text) {
  const forms = [];
  let cursor = 0;
  while (cursor < text.length) {
    const start = text.indexOf('(service', cursor);
    if (start === -1) break;
    const formStart = text.slice(start, start + 80);
    const idMatch = formStart.match(/^\(service\s+:id\s+([^\s\)]+)/);
    if (!idMatch) {
      cursor = start + '(service'.length;
      continue;
    }
    const close = findBalancedClose(text, start);
    if (close === -1) break;
    forms.push({
      serviceId: idMatch[1],
      body: text.slice(start, close + 1),
    });
    cursor = close + 1;
  }
  return forms;
}

function normalizeDeploymentForm(form) {
  return compactObject({
    dc_slug: keywordValue(form, 'dc_slug'),
    substrate: keywordValue(form, 'substrate'),
    runtime_target: keywordValue(form, 'runtime-target') ?? keywordValue(form, 'runtime_target'),
    origin: keywordValue(form, 'origin'),
    tunnel_client: keywordValue(form, 'tunnel-client') ?? keywordValue(form, 'tunnel_client'),
    target_service: keywordValue(form, 'target-service') ?? keywordValue(form, 'target_service'),
    executor: keywordValue(form, 'executor'),
    container: keywordValue(form, 'container') ?? keywordValue(form, 'container_name') ?? keywordValue(form, 'container-name'),
    service: keywordValue(form, 'service'),
    namespace: keywordValue(form, 'namespace'),
    deployment: keywordValue(form, 'deployment'),
    host_bind: keywordValue(form, 'host-bind') ?? keywordValue(form, 'host_bind'),
    local_bind: keywordValue(form, 'local-bind') ?? keywordValue(form, 'local_bind'),
    proxy: keywordValue(form, 'proxy'),
    image_env: keywordValue(form, 'image-env') ?? keywordValue(form, 'image_env'),
    authority: keywordValue(form, 'authority'),
    artifact_delivery_lane: keywordValue(form, 'artifact-delivery-lane')
      ?? keywordValue(form, 'artifact_delivery_lane')
      ?? keywordValue(form, 'artifact-lane')
      ?? keywordValue(form, 'artifact_lane'),
    target_side_build_allowed: boolOrNull(
      keywordValue(form, 'target-side-build-allowed')
      ?? keywordValue(form, 'target_side_build_allowed'),
    ),
    target_side_build_prohibited: boolOrNull(
      keywordValue(form, 'target-side-build-prohibited')
      ?? keywordValue(form, 'target_side_build_prohibited'),
    ),
  });
}

function normalizeProxyForm(form) {
  return compactObject({
    kind: keywordValue(form, 'kind'),
    domain: keywordValue(form, 'domain'),
    upstream: keywordValue(form, 'upstream'),
    routes: keywordListValue(form, 'routes'),
    compat_domain: keywordValue(form, 'compat-domain') ?? keywordValue(form, 'compat_domain'),
    compat_routes: keywordListValue(form, 'compat-routes').concat(keywordListValue(form, 'compat_routes')),
    file: keywordValue(form, 'file'),
    sse_no_buffer: boolOrNull(keywordValue(form, 'sse-no-buffer') ?? keywordValue(form, 'sse_no_buffer')),
    flush_interval: keywordValue(form, 'flush-interval') ?? keywordValue(form, 'flush_interval'),
    read_timeout: keywordValue(form, 'read-timeout') ?? keywordValue(form, 'read_timeout'),
    write_timeout: keywordValue(form, 'write-timeout') ?? keywordValue(form, 'write_timeout'),
    stream_timeout: keywordValue(form, 'stream-timeout') ?? keywordValue(form, 'stream_timeout'),
    route_generation: keywordValue(form, 'route-generation') ?? keywordValue(form, 'route_generation'),
  });
}

function normalizeJarvisRuntimeTopologyForm(form, serviceId) {
  return compactObject({
    schema: keywordValue(form, 'schema') ?? 'missiond.jarvis-runtime-topology.v1',
    service_id: serviceId,
    edge_node: keywordValue(form, 'edge-node') ?? keywordValue(form, 'edge_node'),
    edge_domain: keywordValue(form, 'edge-domain') ?? keywordValue(form, 'edge_domain'),
    edge_public_ip: keywordValue(form, 'edge-public-ip') ?? keywordValue(form, 'edge_public_ip'),
    edge_proxy: keywordValue(form, 'edge-proxy') ?? keywordValue(form, 'edge_proxy'),
    origin_node: keywordValue(form, 'origin-node') ?? keywordValue(form, 'origin_node'),
    origin: keywordValue(form, 'origin'),
    tunnel_server_url: keywordValue(form, 'tunnel-server-url') ?? keywordValue(form, 'tunnel_server_url'),
    tunnel_client_id: keywordValue(form, 'tunnel-client-id') ?? keywordValue(form, 'tunnel_client_id'),
    target_node: keywordValue(form, 'target-node') ?? keywordValue(form, 'target_node'),
    target_service: keywordValue(form, 'target-service') ?? keywordValue(form, 'target_service'),
    target_local_url: keywordValue(form, 'target-local-url') ?? keywordValue(form, 'target_local_url'),
    expected_deploy_agent_version: keywordValue(form, 'expected-deploy-agent-version') ?? keywordValue(form, 'expected_deploy_agent_version'),
    launchd_unit: keywordValue(form, 'launchd-unit') ?? keywordValue(form, 'launchd_unit'),
    launchd_plist: keywordValue(form, 'launchd-plist') ?? keywordValue(form, 'launchd_plist'),
    local_health_url: keywordValue(form, 'local-health-url') ?? keywordValue(form, 'local_health_url'),
    route_generation: keywordValue(form, 'route-generation') ?? keywordValue(form, 'route_generation'),
    proxy_no_buffer: boolOrNull(keywordValue(form, 'proxy-no-buffer') ?? keywordValue(form, 'proxy_no_buffer')),
    proxy_flush_interval: keywordValue(form, 'proxy-flush-interval') ?? keywordValue(form, 'proxy_flush_interval'),
    proxy_read_timeout: keywordValue(form, 'proxy-read-timeout') ?? keywordValue(form, 'proxy_read_timeout'),
    proxy_write_timeout: keywordValue(form, 'proxy-write-timeout') ?? keywordValue(form, 'proxy_write_timeout'),
    proxy_stream_timeout: keywordValue(form, 'proxy-stream-timeout') ?? keywordValue(form, 'proxy_stream_timeout'),
    streaming_policy: keywordValue(form, 'streaming-policy') ?? keywordValue(form, 'streaming_policy'),
    authority: keywordValue(form, 'authority'),
    source_ref: `.missiond/v3/shards/universe/service-runtime.lisp#service:${serviceId}:jarvis-runtime-topology`,
  });
}

function normalizeFrontendDeploymentForm(form) {
  return compactObject({
    substrate: keywordValue(form, 'substrate'),
    project: keywordValue(form, 'project'),
    root_directory: keywordValue(form, 'root-directory') ?? keywordValue(form, 'root_directory'),
    production_domain: keywordValue(form, 'production-domain') ?? keywordValue(form, 'production_domain'),
    fallback_domain: keywordValue(form, 'fallback-domain') ?? keywordValue(form, 'fallback_domain'),
    authority: keywordValue(form, 'authority'),
  });
}

function normalizeBuildLaneForm(form) {
  return compactObject({
    id: keywordValue(form, 'id'),
    builder: keywordValue(form, 'builder'),
    executor: keywordValue(form, 'executor'),
    source_sync: keywordValue(form, 'source-sync') ?? keywordValue(form, 'source_sync'),
    dockerfile: keywordValue(form, 'dockerfile'),
    image: keywordValue(form, 'image'),
    artifact_lane: keywordValue(form, 'artifact-lane') ?? keywordValue(form, 'artifact_lane'),
    manifest: keywordValue(form, 'manifest'),
    authority: keywordValue(form, 'authority'),
    target_side_build_prohibited: boolOrNull(
      keywordValue(form, 'target-side-build-prohibited')
      ?? keywordValue(form, 'target_side_build_prohibited'),
    ),
  });
}

function normalizeExplicitDeploymentChannelsForm(form, serviceId, projectId = serviceId) {
  return extractNamedForms(form, 'channel')
    .map((channelForm, index) => {
      const surface = keywordValue(channelForm, 'surface') ?? 'runtime';
      return compactObject({
        id: keywordValue(channelForm, 'id') ?? `${serviceId}:${surface}:declared:${index}`,
        service_id: keywordValue(channelForm, 'service-id') ?? keywordValue(channelForm, 'service_id') ?? serviceId,
        project_id: keywordValue(channelForm, 'project-id') ?? keywordValue(channelForm, 'project_id') ?? projectId,
        surface,
        channel_kind: keywordValue(channelForm, 'channel-kind') ?? keywordValue(channelForm, 'channel_kind'),
        runner_role: keywordValue(channelForm, 'runner-role') ?? keywordValue(channelForm, 'runner_role'),
        substrate: keywordValue(channelForm, 'substrate'),
        authority: keywordValue(channelForm, 'authority'),
        source_ref: keywordValue(channelForm, 'source-ref') ?? keywordValue(channelForm, 'source_ref') ?? `.missiond/v3/shards/universe/service-runtime.lisp#service:${serviceId}:deployment-channels`,
        workflow: keywordValue(channelForm, 'workflow'),
        deploy_center_slug: keywordValue(channelForm, 'deploy-center-slug') ?? keywordValue(channelForm, 'deploy_center_slug'),
        executor: keywordValue(channelForm, 'executor'),
        builder: keywordValue(channelForm, 'builder'),
        source_sync: keywordValue(channelForm, 'source-sync') ?? keywordValue(channelForm, 'source_sync'),
        dockerfile: keywordValue(channelForm, 'dockerfile'),
        manifest: keywordValue(channelForm, 'manifest'),
        artifact_lane: keywordValue(channelForm, 'artifact-lane') ?? keywordValue(channelForm, 'artifact_lane'),
        image: keywordValue(channelForm, 'image'),
        runtime_target: keywordValue(channelForm, 'runtime-target') ?? keywordValue(channelForm, 'runtime_target'),
        deploy_type: keywordValue(channelForm, 'deploy-type') ?? keywordValue(channelForm, 'deploy_type'),
        stage: keywordValue(channelForm, 'stage'),
        stage_project_slug: keywordValue(channelForm, 'stage-project-slug') ?? keywordValue(channelForm, 'stage_project_slug'),
        vercel_project: keywordValue(channelForm, 'vercel-project') ?? keywordValue(channelForm, 'vercel_project'),
        root_directory: keywordValue(channelForm, 'root-directory') ?? keywordValue(channelForm, 'root_directory'),
        production_domain: keywordValue(channelForm, 'production-domain') ?? keywordValue(channelForm, 'production_domain'),
        target_side_build_prohibited: boolOrNull(
          keywordValue(channelForm, 'target-side-build-prohibited')
          ?? keywordValue(channelForm, 'target_side_build_prohibited'),
        ),
        declared_status: keywordValue(channelForm, 'declared-status') ?? keywordValue(channelForm, 'declared_status') ?? 'declared',
        observed_status: keywordValue(channelForm, 'observed-status') ?? keywordValue(channelForm, 'observed_status') ?? 'not_queried',
        drift_status: keywordValue(channelForm, 'drift-status') ?? keywordValue(channelForm, 'drift_status') ?? 'not_checked',
      });
    });
}

function extractNamedForms(text, name) {
  const forms = [];
  let cursor = 0;
  const marker = `(${name}`;
  while (cursor < text.length) {
    const start = text.indexOf(marker, cursor);
    if (start === -1) break;
    const close = findBalancedClose(text, start);
    if (close === -1) break;
    forms.push(text.slice(start + marker.length, close));
    cursor = close + 1;
  }
  return forms;
}

function parseKeywordForm(text, key) {
  const start = keywordMarkerIndex(text, key);
  if (start === -1) return null;
  let open = text.indexOf('(', start + key.length + 1);
  if (open === -1) return null;
  const close = findBalancedClose(text, open);
  return close === -1 ? null : text.slice(open + 1, close);
}

function keywordMarkerIndex(text, key) {
  const escaped = key.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
  const match = new RegExp(`:${escaped}(?![A-Za-z0-9_-])`).exec(text);
  return match?.index ?? -1;
}

const workflowFactsCache = new Map();
const serviceRegistryCache = new Map();

function readGithubWorkflowFactsForRoot(root) {
  const repoRoot = findAncestorDir(root, '.github/workflows');
  if (!repoRoot) return { byFile: new Map(), byDcSlug: new Map(), genericBuildWorkflows: [] };
  if (workflowFactsCache.has(repoRoot)) return workflowFactsCache.get(repoRoot);
  const workflowsDir = path.join(repoRoot, '.github/workflows');
  const byFile = new Map();
  const byDcSlug = new Map();
  const genericBuildWorkflows = [];
  for (const fileName of safeReaddir(workflowsDir)) {
    if (!fileName.endsWith('.yml') && !fileName.endsWith('.yaml')) continue;
    const file = path.join(workflowsDir, fileName);
    const text = safeRead(file);
    if (!text) continue;
    const fact = compactObject({
      file: fileName,
      path: file,
      source_ref: file,
      service_name: yamlScalar(text, 'service_name'),
      dockerfile: yamlScalar(text, 'dockerfile'),
      image: yamlScalar(text, 'image') ?? workflowEnvValue(text, 'IMAGE'),
      dc_project: yamlScalar(text, 'dc_project') ?? deployCenterTriggerSlug(text),
      uses_reusable_deploy: text.includes('./.github/workflows/reusable-deploy.yml'),
      build_signal: workflowHasBuildSignal(text),
    });
    byFile.set(fileName, fact);
    if (fact.dc_project) byDcSlug.set(fact.dc_project, fact);
    if (fact.build_signal) genericBuildWorkflows.push(fact);
  }
  const result = { repoRoot, byFile, byDcSlug, genericBuildWorkflows };
  workflowFactsCache.set(repoRoot, result);
  return result;
}

function readServiceRegistryForRoot(root) {
  const repoRoot = findAncestorFileRoot(root, 'services.yaml');
  if (!repoRoot) return null;
  const file = path.join(repoRoot, 'services.yaml');
  if (serviceRegistryCache.has(file)) return serviceRegistryCache.get(file);
  const text = safeRead(file);
  if (!text) return null;
  const entries = new Map();
  let current = null;
  for (const rawLine of text.split(/\r?\n/)) {
    const line = rawLine.replace(/\s+#.*$/, '');
    const top = line.match(/^([A-Za-z0-9_-]+):\s*$/);
    if (top) {
      current = { key: top[1], source_ref: `${file}#${top[1]}` };
      entries.set(current.key, current);
      continue;
    }
    if (!current) continue;
    const field = line.match(/^\s{2}([A-Za-z0-9_-]+):\s*(.+?)\s*$/);
    if (field) current[field[1]] = stripYamlScalar(field[2]);
  }
  const byDcSlug = new Map();
  for (const entry of entries.values()) {
    if (entry.dc_slug) byDcSlug.set(entry.dc_slug, entry);
  }
  const registry = { file, entries, byDcSlug };
  serviceRegistryCache.set(file, registry);
  return registry;
}

function matchServiceRegistryEntry(serviceId, deployCenterSlug, registry) {
  if (!registry) return null;
  const aliases = new Map([
    ['xjp-image-service', 'image'],
    ['xjp-video-service', 'video'],
    ['xjp-domain-service', 'domain'],
    ['xjp-mail-service', 'mail'],
    ['xiaojinpro-backend', 'monolith'],
  ]);
  const direct = registry.entries.get(serviceId) ?? registry.entries.get(aliases.get(serviceId));
  if (direct) return direct;
  return deployCenterSlug ? registry.byDcSlug.get(deployCenterSlug) ?? null : null;
}

function selectWorkflowFact({ serviceId, deployCenterSlug, registryEntry, workflowFacts }) {
  if (!workflowFacts) return null;
  const registryWorkflow = registryEntry?.ga_workflow;
  if (registryWorkflow && workflowFacts.byFile.has(registryWorkflow)) {
    return workflowFacts.byFile.get(registryWorkflow);
  }
  if (deployCenterSlug && workflowFacts.byDcSlug.has(deployCenterSlug)) {
    return workflowFacts.byDcSlug.get(deployCenterSlug);
  }
  const preferred = workflowFacts.genericBuildWorkflows.find((workflow) => {
    const file = workflow.file ?? '';
    return file.includes(serviceId) || file.includes(serviceId.replace(/^xjp-/, ''));
  });
  if (preferred) return preferred;
  const releaseWorkflow = workflowFacts.genericBuildWorkflows.find((workflow) => (
    /deploy|publish|release|image/i.test(workflow.file ?? '')
  ));
  if (releaseWorkflow) return releaseWorkflow;
  if (workflowFacts.genericBuildWorkflows.length === 1) return workflowFacts.genericBuildWorkflows[0];
  return null;
}

function findAncestorDir(start, relativeDir) {
  let cursor = path.resolve(start);
  for (let i = 0; i < 8; i += 1) {
    if (fs.existsSync(path.join(cursor, relativeDir))) return cursor;
    const next = path.dirname(cursor);
    if (next === cursor) break;
    cursor = next;
  }
  return null;
}

function findAncestorFileRoot(start, fileName) {
  let cursor = path.resolve(start);
  for (let i = 0; i < 8; i += 1) {
    if (fs.existsSync(path.join(cursor, fileName))) return cursor;
    const next = path.dirname(cursor);
    if (next === cursor) break;
    cursor = next;
  }
  return null;
}

function safeReaddir(dir) {
  try {
    return fs.readdirSync(dir);
  } catch {
    return [];
  }
}

function safeRead(file) {
  try {
    return fs.readFileSync(file, 'utf8');
  } catch {
    return '';
  }
}

function yamlScalar(text, key) {
  const escaped = key.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
  const match = text.match(new RegExp(`^\\s*${escaped}:\\s*([^\\n#]+)`, 'm'));
  return match ? stripYamlScalar(match[1]) : null;
}

function workflowEnvValue(text, key) {
  const escaped = key.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
  const match = text.match(new RegExp(`^\\s*${escaped}:\\s*([^\\n#]+)`, 'm'));
  return match ? stripYamlScalar(match[1]) : null;
}

function stripYamlScalar(value) {
  const trimmed = String(value).trim();
  return trimmed.replace(/^['"]|['"]$/g, '');
}

function deployCenterTriggerSlug(text) {
  const match = text.match(/\/ci\/trigger\/([A-Za-z0-9_.-]+)/);
  return match?.[1] ?? null;
}

function workflowHasBuildSignal(text) {
  return text.includes('./.github/workflows/reusable-deploy.yml')
    || text.includes('docker build')
    || text.includes('docker/build-push-action')
    || text.includes('/ci/trigger/');
}

function findBalancedClose(text, open) {
  let depth = 0;
  let inString = false;
  let escaped = false;
  for (let i = open; i < text.length; i += 1) {
    const ch = text[i];
    if (escaped) {
      escaped = false;
      continue;
    }
    if (ch === '\\') {
      escaped = true;
      continue;
    }
    if (ch === '"') {
      inString = !inString;
      continue;
    }
    if (inString) continue;
    if (ch === '(') depth += 1;
    else if (ch === ')') {
      depth -= 1;
      if (depth === 0) return i;
    }
  }
  return -1;
}

function keywordValue(text, key) {
  const escaped = key.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
  const match = text.match(new RegExp(`:${escaped}\\s+(?:"([^"]+)"|([^\\s\\)]+))`));
  return match?.[1] ?? match?.[2] ?? null;
}

function boolOrNull(value) {
  if (value === 'true' || value === 't') return true;
  if (value === 'false' || value === 'nil') return false;
  return null;
}

function stringArray(value) {
  return Array.isArray(value)
    ? value.filter((item) => typeof item === 'string' && item.trim() !== '')
    : [];
}

function arrayOrEmpty(value) {
  return Array.isArray(value) ? value : [];
}

function stringOrNull(value) {
  return typeof value === 'string' && value.trim() !== '' ? value : null;
}

function positiveIntOrNull(value) {
  return Number.isInteger(value) && value > 0 ? value : null;
}

main();
