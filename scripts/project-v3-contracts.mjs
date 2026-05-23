#!/usr/bin/env node

import fs from 'node:fs';
import path from 'node:path';
import crypto from 'node:crypto';

import { runLispc } from './lib/ocaml_lispc.mjs';

const BLUEPRINT = '.missiond/v3/missiond-blueprint.lisp';
const RUST_OUTPUT = 'crates/missiond-daemon/src/context/v3_contracts/generated.rs';
const JS_OUTPUT = 'scripts/generated/v3_contracts.mjs';
const DTS_OUTPUT = 'scripts/generated/v3_contracts.d.ts';
const RUST_RUNTIME_DEFAULTS_OUTPUT = 'crates/missiond-daemon/src/context/v3_runtime_defaults/generated.rs';
const JS_RUNTIME_DEFAULTS_OUTPUT = 'scripts/generated/v3_runtime_defaults.mjs';

function main() {
  const opts = parseArgs(process.argv.slice(2));
  const repo = path.resolve(opts.repo);
  const blueprint = opts.blueprint;
  const contract = loadContract({ repo, blueprint });
  const runtimeDefaults = loadRuntimeDefaults({ repo, blueprint });
  const projectUniverse = loadProjectUniverse({ repo, blueprint });
  const generated = renderAll(contract, {
    blueprint,
    rustOutput: RUST_OUTPUT,
    jsOutput: JS_OUTPUT,
    dtsOutput: DTS_OUTPUT,
    rustRuntimeDefaultsOutput: RUST_RUNTIME_DEFAULTS_OUTPUT,
    jsRuntimeDefaultsOutput: JS_RUNTIME_DEFAULTS_OUTPUT,
    runtimeConfigSourceHash: runtimeDefaults.sourceDomainHash,
    projectUniverseSourceHash: projectUniverse.sourceDomainHash,
  });
  const generatedRuntimeDefaults = renderRuntimeDefaults(runtimeDefaults, {
    blueprint,
    rustRuntimeDefaultsOutput: RUST_RUNTIME_DEFAULTS_OUTPUT,
    jsRuntimeDefaultsOutput: JS_RUNTIME_DEFAULTS_OUTPUT,
  });
  const outputs = [
    { id: 'rust', rel: opts.rustOutput, text: generated.rust },
    { id: 'js', rel: opts.jsOutput, text: generated.js },
    { id: 'dts', rel: opts.dtsOutput, text: generated.dts },
    { id: 'rust-runtime-defaults', rel: opts.rustRuntimeDefaultsOutput, text: generatedRuntimeDefaults.rust },
    { id: 'js-runtime-defaults', rel: opts.jsRuntimeDefaultsOutput, text: generatedRuntimeDefaults.js },
  ];

  const diagnostics = [];
  if (opts.write) {
    for (const output of outputs) {
      const file = path.join(repo, output.rel);
      fs.mkdirSync(path.dirname(file), { recursive: true });
      fs.writeFileSync(file, output.text);
    }
  }
  if (opts.check) {
    for (const output of outputs) {
      const file = path.join(repo, output.rel);
      let current;
      try {
        current = fs.readFileSync(file, 'utf8');
      } catch (err) {
        diagnostics.push(diag(output.rel, 'GENERATED_CONTRACT_MISSING', `cannot read generated ${output.id}: ${err.message}`));
        continue;
      }
      if (normalizeNewlines(current) !== normalizeNewlines(output.text)) {
        diagnostics.push(diag(output.rel, 'GENERATED_CONTRACT_STALE', `generated ${output.id} contract is stale; run node scripts/project-v3-contracts.mjs --write`));
      }
    }
  }

  const result = {
    ok: diagnostics.length === 0,
    mode: opts.write ? 'write' : opts.check ? 'check' : 'print',
    source_hash: contract.sourceHash,
    outputs: outputs.map((output) => output.rel),
    surfaces: contract.surfaceIds.length,
    functions: contract.functionIds.length,
    artifact_contracts: contract.artifactContractIds.length,
    runtime_policies: contract.runtimePolicies.length,
    source_domains: contract.sourceDomains.length,
    runtime_config_source_hash: runtimeDefaults.sourceDomainHash,
    project_universe_source_hash: projectUniverse.sourceDomainHash,
    checker_commands: checkerCommands(contract).length,
    final_convergence_gate: contract.finalConvergenceGate?.id ?? null,
    diagnostics,
  };

  if (opts.json) {
    console.log(JSON.stringify(result, null, 2));
  } else if (diagnostics.length > 0) {
    for (const d of diagnostics) console.error(`${d.file}: ${d.code}: ${d.message}`);
  } else if (!opts.write && !opts.check) {
    process.stdout.write(generated.js);
  }
  process.exit(diagnostics.length === 0 ? 0 : 1);
}

function parseArgs(argv) {
  const opts = {
    repo: process.cwd(),
    blueprint: BLUEPRINT,
    rustOutput: RUST_OUTPUT,
    jsOutput: JS_OUTPUT,
    dtsOutput: DTS_OUTPUT,
    rustRuntimeDefaultsOutput: RUST_RUNTIME_DEFAULTS_OUTPUT,
    jsRuntimeDefaultsOutput: JS_RUNTIME_DEFAULTS_OUTPUT,
    write: false,
    check: false,
    json: false,
  };
  for (let i = 0; i < argv.length; i += 1) {
    const arg = argv[i];
    if (arg === '--write') opts.write = true;
    else if (arg === '--check') opts.check = true;
    else if (arg === '--json') opts.json = true;
    else if (arg === '--repo') opts.repo = argv[++i] ?? fail('--repo requires a value');
    else if (arg.startsWith('--repo=')) opts.repo = arg.slice('--repo='.length);
    else if (arg === '--blueprint') opts.blueprint = argv[++i] ?? fail('--blueprint requires a value');
    else if (arg.startsWith('--blueprint=')) opts.blueprint = arg.slice('--blueprint='.length);
    else if (arg === '--rust-output') opts.rustOutput = argv[++i] ?? fail('--rust-output requires a value');
    else if (arg.startsWith('--rust-output=')) opts.rustOutput = arg.slice('--rust-output='.length);
    else if (arg === '--js-output') opts.jsOutput = argv[++i] ?? fail('--js-output requires a value');
    else if (arg.startsWith('--js-output=')) opts.jsOutput = arg.slice('--js-output='.length);
    else if (arg === '--dts-output') opts.dtsOutput = argv[++i] ?? fail('--dts-output requires a value');
    else if (arg.startsWith('--dts-output=')) opts.dtsOutput = arg.slice('--dts-output='.length);
    else if (arg === '--rust-runtime-defaults-output') opts.rustRuntimeDefaultsOutput = argv[++i] ?? fail('--rust-runtime-defaults-output requires a value');
    else if (arg.startsWith('--rust-runtime-defaults-output=')) opts.rustRuntimeDefaultsOutput = arg.slice('--rust-runtime-defaults-output='.length);
    else if (arg === '--js-runtime-defaults-output') opts.jsRuntimeDefaultsOutput = argv[++i] ?? fail('--js-runtime-defaults-output requires a value');
    else if (arg.startsWith('--js-runtime-defaults-output=')) opts.jsRuntimeDefaultsOutput = arg.slice('--js-runtime-defaults-output='.length);
    else if (arg === '--help' || arg === '-h') {
      console.log('Usage: node scripts/project-v3-contracts.mjs [--write|--check] [--json] [--repo <path>] [--blueprint <path>]');
      process.exit(0);
    } else {
      fail(`unknown argument: ${arg}`);
    }
  }
  if (opts.write && opts.check) fail('--write and --check are mutually exclusive');
  return opts;
}

function loadContract({ repo, blueprint }) {
  const result = runLispc(['emit-contract-abi', '--blueprint', blueprint], { repoRoot: repo, timeoutMs: 60_000 });
  if (!result?.ok || !result?.compiled) {
    const detail = (result?.diagnostics ?? []).map((d) => d.message ?? JSON.stringify(d)).join('; ');
    fail(`missiond-lispc emit-contract-abi failed${detail ? `: ${detail}` : ''}`);
  }
  const payload = result.compiled.payload ?? {};
  const facts = Array.isArray(payload.facts) ? payload.facts : [];
  return {
    schemaVersion: result.compiled.schema_version,
    sourceHash: result.compiled.source_hash,
    sourceUnits: normalizeSourceUnits(payload.source_units),
    sourceDomains: normalizeSourceDomains(payload.source_domains),
    surfaceIds: uniqueSorted((payload.surfaces ?? []).map((row) => row?.id)),
    functionIds: uniqueSorted((payload.functions ?? []).map((row) => row?.id)),
    artifactContractIds: uniqueSorted(facts.filter((fact) => fact?.kind === 'artifact_contract').map((fact) => fact?.id)),
    runtimePolicies: normalizeRuntimePolicies(payload.runtime_policies, facts),
    checkerRegistry: normalizeCheckerRegistry(payload.checker_registry, facts),
    finalConvergenceGate: normalizeFinalConvergenceGate(facts.find((fact) => fact?.kind === 'final_convergence_gate')),
    productionConsumerBoundaries: normalizeTypedFacts(payload.production_consumer_boundaries, facts, 'production_consumer_boundary'),
    requestStateProjections: normalizeTypedFacts(payload.request_state_projections, facts, 'request_state_projection'),
    scannerPolicies: normalizeTypedFacts(payload.scanner_policies, facts, 'scanner_policy'),
    semanticGates: normalizeTypedFacts(payload.semantic_gates, facts, 'semantic_gate'),
    planContract: payload.plan_contract ?? {},
  };
}

function loadRuntimeDefaults({ repo, blueprint }) {
  const result = runLispc(['emit-runtime-config', '--blueprint', blueprint], { repoRoot: repo, timeoutMs: 60_000 });
  if (!result?.ok || !result?.compiled) {
    const detail = (result?.diagnostics ?? []).map((d) => d.message ?? JSON.stringify(d)).join('; ');
    fail(`missiond-lispc emit-runtime-config failed${detail ? `: ${detail}` : ''}`);
  }
  const payload = result.compiled.payload ?? {};
  const sourceDomains = normalizeSourceDomains(payload.source_domains);
  return {
    schemaVersion: result.compiled.schema_version,
    sourceHash: result.compiled.source_hash,
    sourceDomains,
    sourceDomainHash: sourceDomainBundleHash(sourceDomains),
    payload,
  };
}

function loadProjectUniverse({ repo, blueprint }) {
  const result = runLispc(['emit-universe', '--blueprint', blueprint], { repoRoot: repo, timeoutMs: 60_000 });
  if (!result?.ok || !result?.compiled) {
    const detail = (result?.diagnostics ?? []).map((d) => d.message ?? JSON.stringify(d)).join('; ');
    fail(`missiond-lispc emit-universe failed${detail ? `: ${detail}` : ''}`);
  }
  const payload = result.compiled.payload ?? {};
  const sourceDomains = normalizeSourceDomains(payload.source_domains);
  return {
    schemaVersion: result.compiled.schema_version,
    sourceHash: result.compiled.source_hash,
    sourceDomains,
    sourceDomainHash: sourceDomainBundleHash(sourceDomains),
    payload,
  };
}

function renderAll(contract, labels) {
  return {
    rust: renderRust(contract, labels),
    js: renderJs(contract, labels),
    dts: renderDts(contract, labels),
  };
}

function renderRuntimeDefaults(runtimeDefaults, labels) {
  const json = `${JSON.stringify(runtimeDefaults.payload, null, 2)}\n`;
  return {
    rust: `${header(labels.blueprint, labels.rustRuntimeDefaultsOutput, '//')}

pub const SCHEMA_VERSION: &str = ${rustString(runtimeDefaults.schemaVersion)};
pub const SOURCE_HASH: &str = ${rustString(runtimeDefaults.sourceHash)};
pub const DEFAULT_RUNTIME_CONFIG_JSON: &str = ${rustRawString(json)};
`,
    js: `${header(labels.blueprint, labels.jsRuntimeDefaultsOutput, '//')}

export const SCHEMA_VERSION = ${JSON.stringify(runtimeDefaults.schemaVersion)};
export const SOURCE_HASH = ${JSON.stringify(runtimeDefaults.sourceHash)};
export const DEFAULT_RUNTIME_CONFIG = Object.freeze(${JSON.stringify(runtimeDefaults.payload, null, 2)});
export const DEFAULT_WORKSTATION_RUNTIME_CONFIG = Object.freeze(DEFAULT_RUNTIME_CONFIG.workstation ?? {});
`,
  };
}

function renderRust(contract, labels) {
  return `${header(labels.blueprint, labels.rustOutput, '//')}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SourceUnit {
    pub file: &'static str,
    pub kind: &'static str,
    pub included_by: Option<&'static str>,
    pub include_line: Option<u32>,
    pub source_hash: &'static str,
}

pub const SCHEMA_VERSION: &str = ${rustString(contract.schemaVersion)};
pub const SOURCE_HASH: &str = ${rustString(contract.sourceHash)};
pub const RUNTIME_CONFIG_SOURCE_HASH: &str = ${rustString(labels.runtimeConfigSourceHash)};
pub const PROJECT_UNIVERSE_SOURCE_HASH: &str = ${rustString(labels.projectUniverseSourceHash)};

pub const SOURCE_UNITS: &[SourceUnit] = &[
${contract.sourceUnits.map(renderRustSourceUnit).join('\n')}
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SourceDomain {
    pub id: &'static str,
    pub source_hash: &'static str,
    pub source_units: &'static [SourceUnit],
}

pub const SOURCE_DOMAINS: &[SourceDomain] = &[
${contract.sourceDomains.map(renderRustSourceDomain).join('\n')}
];

${rustStringArrayConst('SURFACE_IDS', contract.surfaceIds)}
${rustStringArrayConst('FUNCTION_IDS', contract.functionIds)}
${rustStringArrayConst('ARTIFACT_CONTRACT_IDS', contract.artifactContractIds)}
${rustStringArrayConst('RUNTIME_POLICY_IDS', contract.runtimePolicies.map((policy) => policy.id))}
${rustStringArrayConst('PRODUCTION_CONSUMER_BOUNDARY_IDS', contract.productionConsumerBoundaries.map((fact) => fact.id))}
${rustStringArrayConst('REQUEST_STATE_PROJECTION_IDS', contract.requestStateProjections.map((fact) => fact.id))}
${rustStringArrayConst('SCANNER_POLICY_IDS', contract.scannerPolicies.map((fact) => fact.id))}
${rustStringArrayConst('SEMANTIC_GATE_IDS', contract.semanticGates.map((fact) => fact.id))}
${rustStringArrayConst('CHECKER_COMMANDS', checkerCommands(contract))}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RuntimePolicyDescriptor {
    pub id: &'static str,
    pub schema_version: &'static str,
    pub form: &'static str,
    pub payload_key: &'static str,
    pub keyword_keys: &'static [&'static str],
    pub nested_forms: &'static [&'static str],
    pub source_file: &'static str,
    pub source_line: u32,
}

pub const RUNTIME_POLICIES: &[RuntimePolicyDescriptor] = &[
${contract.runtimePolicies.map(renderRustRuntimePolicy).join('\n')}
];

pub const PLAN_CONTRACT_SCHEMA_VERSION: &str = ${rustString(contract.planContract.schema_version ?? 'missiond.plan-contract.v1')};
${rustStringArrayConst('PLAN_CONTRACT_ACCEPTED_HEADS', contract.planContract.accepted_heads ?? [])}
${rustStringArrayConst('PLAN_CONTRACT_TOP_LEVEL_HINT_KEYS', contract.planContract.top_level_hint_keys ?? [])}
${rustStringArrayConst('PLAN_CONTRACT_NODE_HINT_KEYS', contract.planContract.node_hint_keys ?? [])}

pub fn is_surface_id(value: &str) -> bool {
    SURFACE_IDS.contains(&value)
}

pub fn is_function_id(value: &str) -> bool {
    FUNCTION_IDS.contains(&value)
}
`;
}

function renderJs(contract, labels) {
  return `${header(labels.blueprint, labels.jsOutput, '//')}

export const SCHEMA_VERSION = ${JSON.stringify(contract.schemaVersion)};
export const SOURCE_HASH = ${JSON.stringify(contract.sourceHash)};
export const RUNTIME_CONFIG_SOURCE_HASH = ${JSON.stringify(labels.runtimeConfigSourceHash)};
export const PROJECT_UNIVERSE_SOURCE_HASH = ${JSON.stringify(labels.projectUniverseSourceHash)};
export const SOURCE_UNITS = Object.freeze(${JSON.stringify(contract.sourceUnits, null, 2)});
export const SOURCE_DOMAINS = Object.freeze(${JSON.stringify(contract.sourceDomains, null, 2)});
export const SURFACE_IDS = Object.freeze(${JSON.stringify(contract.surfaceIds, null, 2)});
export const FUNCTION_IDS = Object.freeze(${JSON.stringify(contract.functionIds, null, 2)});
export const ARTIFACT_CONTRACT_IDS = Object.freeze(${JSON.stringify(contract.artifactContractIds, null, 2)});
export const RUNTIME_POLICIES = Object.freeze(${JSON.stringify(contract.runtimePolicies, null, 2)});
export const RUNTIME_POLICY_IDS = Object.freeze(${JSON.stringify(contract.runtimePolicies.map((policy) => policy.id), null, 2)});
export const CHECKER_REGISTRY = Object.freeze(${JSON.stringify(contract.checkerRegistry, null, 2)});
export const CHECKER_COMMANDS = Object.freeze(${JSON.stringify(checkerCommands(contract), null, 2)});
export const FINAL_CONVERGENCE_GATE = Object.freeze(${JSON.stringify(contract.finalConvergenceGate, null, 2)});
export const PRODUCTION_CONSUMER_BOUNDARIES = Object.freeze(${JSON.stringify(contract.productionConsumerBoundaries, null, 2)});
export const REQUEST_STATE_PROJECTIONS = Object.freeze(${JSON.stringify(contract.requestStateProjections, null, 2)});
export const SCANNER_POLICIES = Object.freeze(${JSON.stringify(contract.scannerPolicies, null, 2)});
export const SEMANTIC_GATES = Object.freeze(${JSON.stringify(contract.semanticGates, null, 2)});
export const PLAN_CONTRACT = Object.freeze(${JSON.stringify(contract.planContract, null, 2)});

const SURFACE_ID_SET = new Set(SURFACE_IDS);
const FUNCTION_ID_SET = new Set(FUNCTION_IDS);

export function isSurfaceId(value) {
  return SURFACE_ID_SET.has(value);
}

export function isFunctionId(value) {
  return FUNCTION_ID_SET.has(value);
}
`;
}

function renderDts(contract, labels) {
  return `${header(labels.blueprint, labels.dtsOutput, '//')}

export interface SourceUnit {
  file: string;
  kind: string;
  included_by: string | null;
  include_line: number | null;
  source_hash: string;
}

export interface SourceDomain {
  id: string;
  source_hash: string;
  source_units: readonly SourceUnit[];
}

export interface RuntimePolicyDescriptor {
  id: string;
  schema_version: string;
  form: string;
  payload_key: string;
  keyword_keys: readonly string[];
  nested_forms: readonly string[];
  source_file: string;
  source_line: number;
}

export interface CheckerRegistryEntry {
  id: string;
  checks: readonly string[];
  source_file: string;
  source_line: number;
}

export interface FinalConvergenceCheck {
  id: string;
  command: string | null;
  argv: readonly string[];
  json: boolean;
  timeoutMs: number;
}

export interface FinalConvergenceGate {
  id: string;
  liveChecks: readonly FinalConvergenceCheck[];
  runtimeChecks: readonly FinalConvergenceCheck[];
  blueprintNeedles: readonly { id: string; needle: string }[];
  facadeBudgets: readonly { id: string; file: string; maxLines: number }[];
  requiredSplitFiles: readonly string[];
  requiredRuntimeFiles: readonly { file: string; needles: readonly string[] }[];
  source: unknown;
}

export interface TypedSemanticFact {
  id: string;
  schemaVersion: string | null;
  surface: string | null;
  owner: string | null;
  authority: string | null;
  inputSchema: string | null;
  status: string | null;
  runtimeConsumers: readonly string[];
  must: readonly string[];
  forbidden: readonly string[];
  allowedContexts: readonly string[];
  requires: readonly string[];
  checker: string | null;
  source: unknown;
}

export type V3SurfaceId = ${tsUnion(contract.surfaceIds)};
export type V3FunctionId = ${tsUnion(contract.functionIds)};
export type V3ArtifactContractId = ${tsUnion(contract.artifactContractIds)};
export type V3RuntimePolicyId = ${tsUnion(contract.runtimePolicies.map((policy) => policy.id))};
export type V3ProductionConsumerBoundaryId = ${tsUnion(contract.productionConsumerBoundaries.map((fact) => fact.id))};
export type V3RequestStateProjectionId = ${tsUnion(contract.requestStateProjections.map((fact) => fact.id))};
export type V3ScannerPolicyId = ${tsUnion(contract.scannerPolicies.map((fact) => fact.id))};
export type V3SemanticGateId = ${tsUnion(contract.semanticGates.map((fact) => fact.id))};

export const SCHEMA_VERSION: ${JSON.stringify(contract.schemaVersion)};
export const SOURCE_HASH: ${JSON.stringify(contract.sourceHash)};
export const RUNTIME_CONFIG_SOURCE_HASH: ${JSON.stringify(labels.runtimeConfigSourceHash)};
export const PROJECT_UNIVERSE_SOURCE_HASH: ${JSON.stringify(labels.projectUniverseSourceHash)};
export const SOURCE_UNITS: readonly SourceUnit[];
export const SOURCE_DOMAINS: readonly SourceDomain[];
export const SURFACE_IDS: readonly V3SurfaceId[];
export const FUNCTION_IDS: readonly V3FunctionId[];
export const ARTIFACT_CONTRACT_IDS: readonly V3ArtifactContractId[];
export const RUNTIME_POLICY_IDS: readonly V3RuntimePolicyId[];
export const RUNTIME_POLICIES: readonly RuntimePolicyDescriptor[];
export const CHECKER_REGISTRY: readonly CheckerRegistryEntry[];
export const CHECKER_COMMANDS: readonly string[];
export const FINAL_CONVERGENCE_GATE: Readonly<FinalConvergenceGate> | null;
export const PRODUCTION_CONSUMER_BOUNDARIES: readonly TypedSemanticFact[];
export const REQUEST_STATE_PROJECTIONS: readonly TypedSemanticFact[];
export const SCANNER_POLICIES: readonly TypedSemanticFact[];
export const SEMANTIC_GATES: readonly TypedSemanticFact[];
export const PLAN_CONTRACT: Readonly<{
  schema_version?: string;
  accepted_heads?: readonly string[];
  top_level_hint_keys?: readonly string[];
  node_hint_keys?: readonly string[];
}>;

export function isSurfaceId(value: string): value is V3SurfaceId;
export function isFunctionId(value: string): value is V3FunctionId;
`;
}

function header(blueprint, output, comment) {
  return `${comment} GENERATED BY scripts/project-v3-contracts.mjs --write
${comment} Source: ${blueprint}
${comment} Output: ${output}
${comment} Do not edit by hand.`;
}

function normalizeSourceUnits(rows) {
  return (Array.isArray(rows) ? rows : []).map((unit) => ({
    file: String(unit?.file ?? ''),
    kind: String(unit?.kind ?? ''),
    included_by: typeof unit?.included_by === 'string' ? unit.included_by : null,
    include_line: Number.isInteger(unit?.include_line) ? unit.include_line : null,
    source_hash: String(unit?.source_hash ?? ''),
  }));
}

function normalizeSourceDomains(rows) {
  return (Array.isArray(rows) ? rows : [])
    .map((domain) => ({
      id: String(domain?.id ?? ''),
      source_hash: String(domain?.source_hash ?? ''),
      source_units: normalizeSourceUnits(domain?.source_units),
    }))
    .filter((domain) => domain.id);
}

function sourceDomainBundleHash(sourceDomains) {
  return crypto
    .createHash('md5')
    .update(sourceDomains.map((domain) => domain.source_hash).join('\n'))
    .digest('hex');
}

function normalizeRuntimePolicies(rows, facts) {
  const candidates = Array.isArray(rows) && rows.length > 0
    ? rows
    : facts.filter((fact) => fact?.kind === 'runtime_policy');
  return uniqueById(candidates.map((row) => ({
    id: stringOrNull(row?.id),
    schema_version: stringOrNull(row?.schema_version) ?? 'missiond.runtime-policy-descriptor.v1',
    form: stringOrNull(row?.form ?? row?.id),
    payload_key: stringOrNull(row?.payload_key) ?? stringOrNull(row?.id),
    keyword_keys: stringArray(row?.keyword_keys),
    nested_forms: stringArray(row?.nested_forms),
    source_file: stringOrNull(row?.source?.source_file) ?? '',
    source_line: numberOrZero(row?.source?.source_line),
  }))).sort((a, b) => a.id.localeCompare(b.id));
}

function normalizeCheckerRegistry(rows, facts) {
  const candidates = Array.isArray(rows) && rows.length > 0
    ? rows
    : facts.filter((fact) => fact?.kind === 'checker_registry');
  return uniqueById(candidates.map((row) => ({
    id: stringOrNull(row?.id),
    checks: stringArray(row?.checks),
    source_file: stringOrNull(row?.source?.source_file) ?? '',
    source_line: numberOrZero(row?.source?.source_line),
  }))).sort((a, b) => a.id.localeCompare(b.id));
}

function normalizeFinalConvergenceGate(row) {
  if (!row || typeof row !== 'object') return null;
  return {
    id: stringOrNull(row?.id) ?? 'v3-final-convergence',
    liveChecks: normalizeGateChecks(row?.live_checks),
    runtimeChecks: normalizeGateChecks(row?.runtime_checks),
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
        maxLines: positiveIntOrNull(entry?.max_lines),
      }))
      .filter((entry) => entry.id && entry.file && entry.maxLines != null),
    requiredSplitFiles: stringArray(row?.required_split_files),
    requiredRuntimeFiles: arrayOrEmpty(row?.required_runtime_files)
      .map((entry) => ({
        file: stringOrNull(entry?.file),
        needles: stringArray(entry?.needles),
      }))
      .filter((entry) => entry.file),
    source: row?.source ?? null,
  };
}

function normalizeTypedFacts(rows, facts, kind) {
  const candidates = Array.isArray(rows) && rows.length > 0
    ? rows
    : facts.filter((fact) => fact?.kind === kind);
  return uniqueById(candidates.map((row) => ({
    id: stringOrNull(row?.id),
    schemaVersion: stringOrNull(row?.schema_version ?? row?.schemaVersion),
    surface: stringOrNull(row?.surface),
    owner: stringOrNull(row?.owner),
    authority: stringOrNull(row?.authority),
    inputSchema: stringOrNull(row?.input_schema ?? row?.inputSchema),
    status: stringOrNull(row?.status),
    runtimeConsumers: stringArray(row?.runtime_consumers ?? row?.runtimeConsumers),
    must: stringArray(row?.must),
    forbidden: stringArray(row?.forbidden),
    allowedContexts: stringArray(row?.allowed_contexts ?? row?.allowedContexts),
    requires: stringArray(row?.requires),
    checker: stringOrNull(row?.checker),
    source: row?.source ?? null,
  }))).sort((a, b) => a.id.localeCompare(b.id));
}

function normalizeGateChecks(rows) {
  return arrayOrEmpty(rows)
    .map((entry) => ({
      id: stringOrNull(entry?.id),
      command: stringOrNull(entry?.command),
      argv: stringArray(entry?.argv),
      json: entry?.json === true,
      timeoutMs: positiveIntOrNull(entry?.timeout_ms) ?? 60_000,
    }))
    .filter((entry) => entry.id && entry.argv.length > 0);
}

function checkerCommands(contract) {
  return uniqueSorted((contract.checkerRegistry ?? []).flatMap((entry) => entry.checks));
}

function uniqueById(rows) {
  const seen = new Set();
  const out = [];
  for (const row of rows) {
    if (!row.id || seen.has(row.id)) continue;
    seen.add(row.id);
    out.push(row);
  }
  return out;
}

function uniqueSorted(values) {
  return [...new Set(values.filter((value) => typeof value === 'string' && value.length > 0))].sort();
}

function stringOrNull(value) {
  return typeof value === 'string' && value.trim() !== '' ? value : null;
}

function stringArray(value) {
  return Array.isArray(value)
    ? value.filter((item) => typeof item === 'string' && item.trim() !== '')
    : [];
}

function arrayOrEmpty(value) {
  return Array.isArray(value) ? value : [];
}

function positiveIntOrNull(value) {
  return Number.isInteger(value) && value > 0 ? value : null;
}

function numberOrZero(value) {
  return Number.isInteger(value) && value >= 0 ? value : 0;
}

function renderRustSourceUnit(unit) {
  return renderRustSourceUnitWithIndent(unit, '    ', true);
}

function renderRustSourceDomain(domain) {
  const sourceUnits = renderRustSourceDomainUnits(domain.source_units);
  return `    SourceDomain {
        id: ${rustString(domain.id)},
        source_hash: ${rustString(domain.source_hash)},
        source_units: ${sourceUnits},
    },`;
}

function renderRustSourceDomainUnits(units) {
  if (!units.length) return '&[]';
  if (units.length === 1) {
    return `&[SourceUnit {
            file: ${rustString(units[0].file)},
            kind: ${rustString(units[0].kind)},
            included_by: ${rustOptionString(units[0].included_by)},
            include_line: ${rustOptionU32(units[0].include_line)},
            source_hash: ${rustString(units[0].source_hash)},
        }]`;
  }
  return `&[
${units.map((unit) => renderRustSourceUnitWithIndent(unit, '            ', true)).join('\n')}
        ]`;
}

function renderRustSourceUnitWithIndent(unit, indent, trailingComma) {
  return `${indent}SourceUnit {
${indent}    file: ${rustString(unit.file)},
${indent}    kind: ${rustString(unit.kind)},
${indent}    included_by: ${rustOptionString(unit.included_by)},
${indent}    include_line: ${rustOptionU32(unit.include_line)},
${indent}    source_hash: ${rustString(unit.source_hash)},
${indent}}${trailingComma ? ',' : ''}`;
}

function rustStringArrayConst(name, values) {
  if (!values.length) return `pub const ${name}: &[&str] = &[];`;
  const arrayInline = `&[${values.map(rustString).join(', ')}]`;
  const inline = `pub const ${name}: &[&str] = ${arrayInline};`;
  if (inline.length <= 100) return inline;
  if (arrayInline.length <= 100) {
    return `pub const ${name}: &[&str] =
    ${arrayInline};`;
  }
  return `pub const ${name}: &[&str] = &[
${values.map((value) => `    ${rustString(value)},`).join('\n')}
];`;
}

function renderRustRuntimePolicy(policy) {
  return `    RuntimePolicyDescriptor {
        id: ${rustString(policy.id)},
        schema_version: ${rustString(policy.schema_version)},
        form: ${rustString(policy.form)},
        payload_key: ${rustString(policy.payload_key)},
        keyword_keys: ${rustStringSlice(policy.keyword_keys, '            ', '        ')},
        nested_forms: ${rustStringSlice(policy.nested_forms, '            ', '        ')},
        source_file: ${rustString(policy.source_file)},
        source_line: ${policy.source_line}u32,
    },`;
}

function rustStringSlice(values, itemIndent, closingIndent) {
  if (!values.length) return '&[]';
  const inline = `&[${values.map(rustString).join(', ')}]`;
  if (inline.length <= 80) return inline;
  return `&[
${values.map((value) => `${itemIndent}${rustString(value)},`).join('\n')}
${closingIndent}]`;
}

function rustString(value) {
  return JSON.stringify(String(value));
}

function rustRawString(value) {
  let hashes = '';
  while (String(value).includes(`"${hashes}`)) hashes += '#';
  return `r${hashes}"${value}"${hashes}`;
}

function rustOptionString(value) {
  return typeof value === 'string' ? `Some(${rustString(value)})` : 'None';
}

function rustOptionU32(value) {
  return Number.isInteger(value) ? `Some(${value}u32)` : 'None';
}

function tsUnion(values) {
  return values.length > 0 ? values.map((value) => JSON.stringify(value)).join(' | ') : 'string';
}

function normalizeNewlines(text) {
  return String(text).replace(/\r\n/g, '\n');
}

function diag(file, code, message) {
  return { file, line: 1, column: 1, code, message, path: '' };
}

function fail(message) {
  console.error(message);
  process.exit(2);
}

main();
