#!/usr/bin/env node

import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { runLispc } from './lib/ocaml_lispc.mjs';
import { buildAgentSlices, buildProjectAgentNavigation } from './lib/v3_agent_slices.mjs';
import { runSemanticRules } from './lib/v3_semantic_rules.mjs';
import { RUNTIME_DOMAIN_SPECS } from './lib/v3_runtime_domains.mjs';
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
    const compiled = bundleTargets[target.id];
    if (!compiled || !targetCompiledOk(compiled)) {
      results.push({
        id: target.id,
        ok: false,
        diagnostics: compiled?.diagnostics ?? bundle?.diagnostics ?? [],
        stderr: bundle?.stderr ?? '',
      });
      continue;
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
    rows.push({
      project_id: projectId,
      service_id: serviceId,
      deploy_center_slug: deployment.dc_slug ?? (deployment.substrate === 'deploy-center' ? serviceId : null),
      domains: stringArray(service?.domains),
      aliases: stringArray(project?.aliases),
      maturity,
      environment,
      manifest_required: strict,
      immutable_image_required: strict,
      runtime_digest_required: strict,
      smoke_required: strict,
      db_adoption_required: serviceId.includes('payments') || projectId.includes('payments'),
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
        },
      },
      policies: rows,
      source_units: Array.isArray(payload.source_units) ? payload.source_units : [],
      source_domains: Array.isArray(payload.source_domains) ? payload.source_domains : [],
    },
  };
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
  const file = '.missiond/v3/shards/universe/service-runtime.lisp';
  const map = new Map();
  let text = '';
  try {
    text = fs.readFileSync(file, 'utf8');
  } catch {
    return map;
  }
  const serviceRe = /\(service\s+:id\s+([^\s\)]+)([\s\S]*?)(?=\n\s*\(service\s+:id\s+|\n\s*\)\s*\)\s*$)/g;
  for (const serviceMatch of text.matchAll(serviceRe)) {
    const serviceId = serviceMatch[1];
    const body = serviceMatch[2];
    const deploymentMatch = body.match(/:deployment\s+\(([^\)]*)\)/);
    if (!deploymentMatch) continue;
    const deployment = deploymentMatch[1];
    const dcSlug = deployment.match(/:dc_slug\s+"([^"]+)"/)?.[1] ?? null;
    const substrate = deployment.match(/:substrate\s+([^\s\)]+)/)?.[1] ?? null;
    map.set(serviceId, { dc_slug: dcSlug, substrate });
  }
  return map;
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
