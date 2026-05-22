import fs from 'node:fs';
import crypto from 'node:crypto';
import path from 'node:path';

import {
  head,
  isList,
  keywordPropText,
  nodeText,
  nodeToStringArray,
  parseLisp,
  readKeywordProps,
} from './missiond_lisp.mjs';
import { readBlueprintResolvedSource } from './v3_blueprint_contract_source.mjs';

export const DEFAULT_MODEL_PROFILE = 'coding-default-opus-4-7';
export const DEFAULT_TIMEOUT_SECS = 1800;
export const MIN_TIMEOUT_SECS = 60;
export const MAX_TIMEOUT_SECS = 7200;
export const DEFAULT_CC_SWARM_TIMEOUT_SECS = 600;
export const MIN_CC_SWARM_TIMEOUT_SECS = 60;
export const MAX_CC_SWARM_TIMEOUT_SECS = 7200;
export const DEFAULT_PTY_SEND_TIMEOUT_SECS = 300;
export const MIN_PTY_SEND_TIMEOUT_SECS = 1;
export const MAX_PTY_SEND_TIMEOUT_SECS = 7200;
export const DEFAULT_DYNAMIC_SLOT_SPAWN_TIMEOUT_SECS = 60;
export const MIN_DYNAMIC_SLOT_SPAWN_TIMEOUT_SECS = 10;
export const MAX_DYNAMIC_SLOT_SPAWN_TIMEOUT_SECS = 600;
export const DEFAULT_CONTEXT_PACK_MAX_PARALLEL = 4;
export const MIN_CONTEXT_PACK_MAX_PARALLEL = 1;
export const MAX_CONTEXT_PACK_MAX_PARALLEL = 8;
export const WATCHDOG_GRACE_SECS = 120;
export const MISSING_SESSION_PROBE_SECS = 120;
export const DEFAULT_SLOT_TTL_SECS = 14400;
export const MIN_SLOT_TTL_SECS = 300;
export const MAX_SLOT_TTL_SECS = 28800;
export const DEFAULT_SLOT_EXTEND_SECS = 3600;
export const MAX_SLOT_EXTEND_SECS = 3600;
export const DEFAULT_SLOT_DEFAULT_CWD = '/Users/jinchen/Projects';
export const DEFAULT_SLOT_MCP_CONFIG = '/Users/jinchen/.xjp-mission/xjp-mcp-config.json';
export const DEFAULT_ALLOWED_CWD_PREFIXES = [
  '/Users/jinchen/Projects',
  '/Users/jinchen/Downloads',
  '/Users/jinchen/Documents',
  '/tmp',
];

const PROFILE_RE = /^[A-Za-z0-9][A-Za-z0-9._-]*$/;
const COMPILED_RUNTIME_CONFIG_REL = path.join(
  '.missiond',
  'v3',
  'runtime',
  'compiled',
  'compiled-runtime-config.json',
);
const V3_ALLOW_SOURCE_FALLBACK_ENV = 'MISSIOND_V3_ALLOW_SOURCE_FALLBACK';
const V3_COMPILE_RUNTIME_ACTION = 'node scripts/compile-v3-runtime.mjs --json';

export class V3BlueprintRuntimeConfigError extends Error {
  constructor(message) {
    super(message);
    this.name = 'V3BlueprintRuntimeConfigError';
    this.code = 'V3_BLUEPRINT_CONFIG_ERROR';
  }
}

export class WorkstationRuntimeConfig {
  constructor({
    slotDefaultProfiles = defaultSlotProfiles(),
    slotTemplates = defaultSlotTemplates(),
    allowedCwdPrefixes = DEFAULT_ALLOWED_CWD_PREFIXES,
    timeoutPolicy = defaultTimeoutPolicy(),
    ccSwarmTimeoutPolicy = defaultCcSwarmTimeoutPolicy(),
    ptySendTimeoutPolicy = defaultPtySendTimeoutPolicy(),
    dynamicSlotSpawnTimeoutPolicy = defaultDynamicSlotSpawnTimeoutPolicy(),
    contextPackDispatchPolicy = defaultContextPackDispatchPolicy(),
    slotTtlPolicy = defaultSlotTtlPolicy(),
    source = 'defaults',
    diagnostics = [],
  } = {}) {
    this.slotDefaultProfiles = new Map(slotDefaultProfiles);
    this.slotTemplates = new Map(slotTemplates);
    this.allowedCwdPrefixes = [...allowedCwdPrefixes];
    this.timeoutPolicy = { ...timeoutPolicy };
    this.ccSwarmTimeoutPolicy = { ...ccSwarmTimeoutPolicy };
    this.ptySendTimeoutPolicy = { ...ptySendTimeoutPolicy };
    this.dynamicSlotSpawnTimeoutPolicy = { ...dynamicSlotSpawnTimeoutPolicy };
    this.contextPackDispatchPolicy = { ...contextPackDispatchPolicy };
    this.slotTtlPolicy = { ...slotTtlPolicy };
    this.source = source;
    this.diagnostics = [...diagnostics];
  }

  defaultModelProfileForTemplate(template) {
    return this.slotDefaultProfiles.get(template) ?? null;
  }

  slotTemplate(template) {
    return this.slotTemplates.get(template) ?? null;
  }

  availableSlotTemplateNames() {
    return [...this.slotTemplates.keys()].sort();
  }

  clampTimeoutSecs(timeoutSecs = null) {
    const raw = Number.isInteger(timeoutSecs) && timeoutSecs > 0
      ? timeoutSecs
      : this.timeoutPolicy.default_secs;
    return Math.max(this.timeoutPolicy.min_secs, Math.min(this.timeoutPolicy.max_secs, raw));
  }

  clampCcSwarmTimeoutMs(timeoutMs = null) {
    const raw = Number.isInteger(timeoutMs) && timeoutMs > 0
      ? timeoutMs
      : this.ccSwarmTimeoutPolicy.default_secs * 1000;
    return Math.max(
      this.ccSwarmTimeoutPolicy.min_secs * 1000,
      Math.min(this.ccSwarmTimeoutPolicy.max_secs * 1000, raw),
    );
  }

  clampPtySendTimeoutMs(timeoutMs = null) {
    const raw = Number.isInteger(timeoutMs) && timeoutMs > 0
      ? timeoutMs
      : this.ptySendTimeoutPolicy.default_secs * 1000;
    return Math.max(
      this.ptySendTimeoutPolicy.min_secs * 1000,
      Math.min(this.ptySendTimeoutPolicy.max_secs * 1000, raw),
    );
  }

  dynamicSlotSpawnTimeoutSecs() {
    return Math.max(
      1,
      Math.max(
        this.dynamicSlotSpawnTimeoutPolicy.min_secs,
        Math.min(
          this.dynamicSlotSpawnTimeoutPolicy.max_secs,
          this.dynamicSlotSpawnTimeoutPolicy.default_secs,
        ),
      ),
    );
  }

  contextPackMaxParallel(maxParallel = null) {
    if (maxParallel === 'all') return 'all';
    const parsed = Number.isInteger(maxParallel) && maxParallel > 0
      ? maxParallel
      : typeof maxParallel === 'string' && /^[1-9][0-9]*$/.test(maxParallel)
        ? Number.parseInt(maxParallel, 10)
        : null;
    const raw = parsed ?? this.contextPackDispatchPolicy.default_max_parallel;
    const clamped = Math.max(
      this.contextPackDispatchPolicy.min_parallel,
      Math.min(this.contextPackDispatchPolicy.max_parallel, raw),
    );
    return String(clamped);
  }

  clampSlotTtlSecs(ttlSecs = null) {
    const raw = Number.isInteger(ttlSecs) && ttlSecs > 0
      ? ttlSecs
      : this.slotTtlPolicy.default_secs;
    return Math.max(this.slotTtlPolicy.min_secs, Math.min(this.slotTtlPolicy.max_secs, raw));
  }

  defaultSlotExtendSecs() {
    return Math.max(
      this.slotTtlPolicy.min_secs,
      Math.min(this.maxSlotExtendSecs(), this.slotTtlPolicy.default_extend_secs),
    );
  }

  maxSlotExtendSecs() {
    return Math.max(
      this.slotTtlPolicy.min_secs,
      Math.min(this.slotTtlPolicy.max_secs, this.slotTtlPolicy.max_extend_secs),
    );
  }
}

export function loadWorkstationRuntimeConfigForRepo(
  repoRoot = process.cwd(),
  { blueprintPath = null, allowDefaultFallback = false, allowSourceFallback = null } = {},
) {
  const repo = path.resolve(repoRoot);
  const explicitBlueprint = blueprintPath != null;
  const resolvedBlueprint = explicitBlueprint
    ? path.resolve(repo, blueprintPath)
    : path.join(repo, '.missiond', 'v3', 'missiond-blueprint.lisp');
  const missiondDir = path.join(repo, '.missiond');
  const fallbackAllowed = allowSourceFallback ?? v3SourceFallbackAllowed();
  let compiledDiagnostics = [];

  if (!explicitBlueprint) {
    const compiled = loadCompiledWorkstationRuntimeConfig(repo);
    compiledDiagnostics = compiled.diagnostics;
    if (compiled.config) return compiled.config;
  }
  const fallbackDiagnostic = explicitBlueprint
    ? 'explicit blueprint requested; parsing raw Lisp workstation config'
    : 'compiled runtime config unavailable or stale; explicit source Lisp fallback enabled';

  if (!fs.existsSync(resolvedBlueprint)) {
    if ((explicitBlueprint || fs.existsSync(missiondDir)) && !allowDefaultFallback) {
      throw new V3BlueprintRuntimeConfigError(`V3 blueprint missing at ${resolvedBlueprint}`);
    }
    return defaultWorkstationRuntimeConfig('fallback-defaults');
  }
  if (!explicitBlueprint && !fallbackAllowed) {
    const detail = compiledDiagnostics.length > 0
      ? compiledDiagnostics.join('; ')
      : 'compiled runtime config payload missing';
    throw new V3BlueprintRuntimeConfigError(
      `MissionD V3 blueprint exists at ${resolvedBlueprint}; compiled runtime config is required but unavailable or invalid: ${detail}. Required action: ${V3_COMPILE_RUNTIME_ACTION}. Set ${V3_ALLOW_SOURCE_FALLBACK_ENV}=1 only for explicit development/test source fallback.`,
    );
  }

  let source;
  try {
    source = readBlueprintResolvedSource(repo, resolvedBlueprint);
  } catch (err) {
    throw new V3BlueprintRuntimeConfigError(
      `failed to load resolved V3 blueprint ${resolvedBlueprint}: ${err.message}`,
    );
  }
  const config = parseWorkstationRuntimeConfig(source, resolvedBlueprint);
  config.diagnostics.push(fallbackDiagnostic);
  return config;
}

function loadCompiledWorkstationRuntimeConfig(repo) {
  const compiledPath = path.join(repo, COMPILED_RUNTIME_CONFIG_REL);
  const diagnostics = [];
  if (!fs.existsSync(compiledPath)) {
    return {
      config: null,
      diagnostics: [`compiled runtime config missing: ${compiledPath}`],
    };
  }
  try {
    const compiled = JSON.parse(fs.readFileSync(compiledPath, 'utf8'));
    if (compiled?.schema_version !== 'missiond.compiled-runtime-config.v1') {
      diagnostics.push(`compiled runtime config has unsupported schema_version ${JSON.stringify(compiled?.schema_version)}`);
    }
    if (Array.isArray(compiled?.diagnostics) && compiled.diagnostics.length > 0) {
      diagnostics.push(`compiled runtime config contains diagnostics: ${compiled.diagnostics.map((d) => d?.message ?? String(d)).join('; ')}`);
    }
    diagnostics.push(...validateCompiledSourceUnits(repo, compiled));
    const workstation = compiled?.payload?.workstation;
    if (!workstation || typeof workstation !== 'object') {
      diagnostics.push('compiled runtime config payload.workstation missing');
    }
    if (diagnostics.length > 0) {
      return { config: null, diagnostics };
    }
    return {
      config: workstationConfigFromCompiled(workstation, compiledPath),
      diagnostics: [],
    };
  } catch (err) {
    return {
      config: null,
      diagnostics: [`failed to load compiled runtime config ${compiledPath}: ${err.message}`],
    };
  }
}

function validateCompiledSourceUnits(repo, compiled) {
  const diagnostics = [];
  const sourceUnits = compiled?.payload?.source_units;
  if (!Array.isArray(sourceUnits) || sourceUnits.length === 0) {
    return ['compiled runtime config missing payload.source_units'];
  }

  const unitHashes = [];
  for (const unit of sourceUnits) {
    const file = typeof unit?.file === 'string' ? unit.file : '';
    const expectedHash = typeof unit?.source_hash === 'string' ? unit.source_hash : '';
    if (!file) {
      diagnostics.push('compiled runtime config contains a source_unit with an empty file');
      continue;
    }
    if (!expectedHash) {
      diagnostics.push(`compiled runtime config source_unit ${file} missing source_hash`);
      continue;
    }
    const sourcePath = path.isAbsolute(file) ? file : path.join(repo, file);
    let actualHash;
    try {
      actualHash = md5Hex(fs.readFileSync(sourcePath));
    } catch (err) {
      diagnostics.push(`compiled runtime config source_units reference unreadable source ${sourcePath}: ${err.message}`);
      continue;
    }
    if (actualHash !== expectedHash) {
      diagnostics.push(`compiled runtime config source_units stale for ${file}: expected ${expectedHash}, got ${actualHash}`);
    }
    unitHashes.push(expectedHash);
  }

  if (diagnostics.length === 0) {
    const actualCompositeHash = md5Hex(Buffer.from(unitHashes.join('\n'), 'utf8'));
    if (actualCompositeHash !== compiled?.source_hash) {
      diagnostics.push(
        `compiled runtime config source_hash mismatch from source_units: expected ${compiled?.source_hash ?? '<missing>'}, got ${actualCompositeHash}`,
      );
    }
  }
  return diagnostics;
}

function md5Hex(bytes) {
  return crypto.createHash('md5').update(bytes).digest('hex');
}

function v3SourceFallbackAllowed() {
  if (process.env.NODE_ENV === 'production') return false;
  const value = process.env[V3_ALLOW_SOURCE_FALLBACK_ENV];
  return /^(1|true|yes|on)$/i.test(String(value ?? '').trim());
}

function workstationConfigFromCompiled(raw, source) {
  return new WorkstationRuntimeConfig({
    slotDefaultProfiles: objectMapEntries(raw.slot_default_profiles),
    slotTemplates: objectMapEntries(raw.slot_templates),
    allowedCwdPrefixes: Array.isArray(raw.allowed_cwd_prefixes)
      ? raw.allowed_cwd_prefixes
      : DEFAULT_ALLOWED_CWD_PREFIXES,
    timeoutPolicy: raw.timeout_policy ?? defaultTimeoutPolicy(),
    ccSwarmTimeoutPolicy: raw.cc_swarm_timeout_policy ?? defaultCcSwarmTimeoutPolicy(),
    ptySendTimeoutPolicy: raw.pty_send_timeout_policy ?? defaultPtySendTimeoutPolicy(),
    dynamicSlotSpawnTimeoutPolicy:
      raw.dynamic_slot_spawn_timeout_policy ?? defaultDynamicSlotSpawnTimeoutPolicy(),
    contextPackDispatchPolicy:
      raw.context_pack_dispatch_policy ?? defaultContextPackDispatchPolicy(),
    slotTtlPolicy: raw.slot_ttl_policy ?? defaultSlotTtlPolicy(),
    source,
    diagnostics: [],
  });
}

function objectMapEntries(value) {
  if (!value || typeof value !== 'object' || Array.isArray(value)) return [];
  return Object.entries(value);
}

export function parseWorkstationRuntimeConfig(source, file = '<memory>') {
  let forms;
  try {
    forms = parseLisp(source, file);
  } catch (err) {
    throw new V3BlueprintRuntimeConfigError(
      `failed to parse V3 blueprint ${file}: ${err.message}`,
    );
  }

  const block = findFirstListByHead(forms, 'workstation-config');
  if (!block) {
    throw new V3BlueprintRuntimeConfigError(
      'failed to parse V3 workstation-config: missing (workstation-config ...)',
    );
  }

  const config = defaultWorkstationRuntimeConfig(file);
  const slotTemplateForms = block.children.filter(
    (child) => isList(child) && head(child) === 'slot-template',
  );
  if (slotTemplateForms.length > 0) {
    config.slotDefaultProfiles.clear();
    config.slotTemplates.clear();
  }
  for (const child of slotTemplateForms) {
    const template = nodeText(child.children[1]);
    if (!template) continue;
    const props = readKeywordProps(child, { start: 2 });
    const profile = keywordPropText(props, ':default-model-profile');
    if (profile) {
      validateProfile(profile, `${file}: slot-template ${template}`);
      config.slotDefaultProfiles.set(template, profile);
    }
    config.slotTemplates.set(template, {
      name: template,
      role: requiredText(props, ':role', `${file}: slot-template ${template}`),
      description: keywordPropText(props, ':description') ?? `Dynamic ${template} slot`,
      default_model_profile: profile ?? null,
      mcp_config: keywordPropText(props, ':mcp-config') ?? null,
      default_cwd: keywordPropText(props, ':default-cwd') ?? DEFAULT_SLOT_DEFAULT_CWD,
    });
  }

  const cwdPolicyForm = block.children.find((child) => {
    if (!isList(child) || head(child) !== 'cwd-policy') return false;
    return nodeText(child.children[1]) === 'dynamic-slot';
  });
  if (cwdPolicyForm) {
    const cwdProps = readKeywordProps(cwdPolicyForm, { start: 2 });
    const allowed = keywordPropList(cwdProps, ':allowed-prefixes');
    if (allowed.length > 0) config.allowedCwdPrefixes = allowed;
  }

  const timeoutForm = block.children.find((child) => {
    if (!isList(child) || head(child) !== 'timeout-policy') return false;
    return nodeText(child.children[1]) === 'boardtask-dispatch';
  });
  if (!timeoutForm) {
    throw new V3BlueprintRuntimeConfigError(
      'failed to parse V3 workstation-config: missing (timeout-policy boardtask-dispatch ...)',
    );
  }
  const props = readKeywordProps(timeoutForm, { start: 2 });
  config.timeoutPolicy = {
    default_secs: readPositiveInt(props, ':default_secs', file),
    min_secs: readPositiveInt(props, ':min_secs', file),
    max_secs: readPositiveInt(props, ':max_secs', file),
    watchdog_grace_secs: readPositiveInt(props, ':watchdog_grace_secs', file),
    missing_session_probe_secs: readPositiveInt(props, ':missing_session_probe_secs', file),
  };
  const ccSwarmTimeoutForm = block.children.find((child) => {
    if (!isList(child) || head(child) !== 'timeout-policy') return false;
    return nodeText(child.children[1]) === 'claudecode-swarm';
  });
  if (!ccSwarmTimeoutForm) {
    throw new V3BlueprintRuntimeConfigError(
      'failed to parse V3 workstation-config: missing (timeout-policy claudecode-swarm ...)',
    );
  }
  const ccSwarmProps = readKeywordProps(ccSwarmTimeoutForm, { start: 2 });
  config.ccSwarmTimeoutPolicy = {
    default_secs: readPositiveInt(ccSwarmProps, ':default_secs', file),
    min_secs: readPositiveInt(ccSwarmProps, ':min_secs', file),
    max_secs: readPositiveInt(ccSwarmProps, ':max_secs', file),
  };
  const ptySendTimeoutForm = block.children.find((child) => {
    if (!isList(child) || head(child) !== 'timeout-policy') return false;
    return nodeText(child.children[1]) === 'pty-send-blocking';
  });
  if (!ptySendTimeoutForm) {
    throw new V3BlueprintRuntimeConfigError(
      'failed to parse V3 workstation-config: missing (timeout-policy pty-send-blocking ...)',
    );
  }
  const ptySendProps = readKeywordProps(ptySendTimeoutForm, { start: 2 });
  config.ptySendTimeoutPolicy = {
    default_secs: readPositiveInt(ptySendProps, ':default_secs', file),
    min_secs: readPositiveInt(ptySendProps, ':min_secs', file),
    max_secs: readPositiveInt(ptySendProps, ':max_secs', file),
  };
  const dynamicSlotSpawnTimeoutForm = block.children.find((child) => {
    if (!isList(child) || head(child) !== 'timeout-policy') return false;
    return nodeText(child.children[1]) === 'dynamic-slot-spawn';
  });
  if (dynamicSlotSpawnTimeoutForm) {
    const dynamicSlotSpawnProps = readKeywordProps(dynamicSlotSpawnTimeoutForm, { start: 2 });
    config.dynamicSlotSpawnTimeoutPolicy = {
      default_secs: readPositiveInt(dynamicSlotSpawnProps, ':default_secs', file),
      min_secs: readPositiveInt(dynamicSlotSpawnProps, ':min_secs', file),
      max_secs: readPositiveInt(dynamicSlotSpawnProps, ':max_secs', file),
    };
  }
  const contextPackDispatchForm = block.children.find((child) => {
    if (!isList(child) || head(child) !== 'dispatch-policy') return false;
    return nodeText(child.children[1]) === 'context-pack-run-wave';
  });
  if (contextPackDispatchForm) {
    const dispatchProps = readKeywordProps(contextPackDispatchForm, { start: 2 });
    config.contextPackDispatchPolicy = {
      default_max_parallel: readPositiveInt(dispatchProps, ':default_max_parallel', file),
      min_parallel: readPositiveInt(dispatchProps, ':min_parallel', file),
      max_parallel: readPositiveInt(dispatchProps, ':max_parallel', file),
    };
  }
  const ttlForm = block.children.find((child) => {
    if (!isList(child) || head(child) !== 'ttl-policy') return false;
    return nodeText(child.children[1]) === 'dynamic-slot';
  });
  if (!ttlForm) {
    throw new V3BlueprintRuntimeConfigError(
      'failed to parse V3 workstation-config: missing (ttl-policy dynamic-slot ...)',
    );
  }
  const ttlProps = readKeywordProps(ttlForm, { start: 2 });
  config.slotTtlPolicy = {
    default_secs: readPositiveInt(ttlProps, ':default_secs', file),
    min_secs: readPositiveInt(ttlProps, ':min_secs', file),
    max_secs: readPositiveInt(ttlProps, ':max_secs', file),
    default_extend_secs: readPositiveInt(ttlProps, ':default_extend_secs', file),
    max_extend_secs: readPositiveInt(ttlProps, ':max_extend_secs', file),
  };
  if (config.timeoutPolicy.min_secs > config.timeoutPolicy.max_secs) {
    throw new V3BlueprintRuntimeConfigError(
      'failed to parse V3 workstation-config: :min_secs exceeds :max_secs',
    );
  }
  if (config.ccSwarmTimeoutPolicy.min_secs > config.ccSwarmTimeoutPolicy.max_secs) {
    throw new V3BlueprintRuntimeConfigError(
      'failed to parse V3 workstation-config: claudecode-swarm :min_secs exceeds :max_secs',
    );
  }
  if (
    config.ccSwarmTimeoutPolicy.default_secs < config.ccSwarmTimeoutPolicy.min_secs
    || config.ccSwarmTimeoutPolicy.default_secs > config.ccSwarmTimeoutPolicy.max_secs
  ) {
    throw new V3BlueprintRuntimeConfigError(
      'failed to parse V3 workstation-config: claudecode-swarm :default_secs outside :min_secs..:max_secs',
    );
  }
  if (config.ptySendTimeoutPolicy.min_secs > config.ptySendTimeoutPolicy.max_secs) {
    throw new V3BlueprintRuntimeConfigError(
      'failed to parse V3 workstation-config: pty-send-blocking :min_secs exceeds :max_secs',
    );
  }
  if (
    config.ptySendTimeoutPolicy.default_secs < config.ptySendTimeoutPolicy.min_secs
    || config.ptySendTimeoutPolicy.default_secs > config.ptySendTimeoutPolicy.max_secs
  ) {
    throw new V3BlueprintRuntimeConfigError(
      'failed to parse V3 workstation-config: pty-send-blocking :default_secs outside :min_secs..:max_secs',
    );
  }
  if (
    config.dynamicSlotSpawnTimeoutPolicy.default_secs < config.dynamicSlotSpawnTimeoutPolicy.min_secs
    || config.dynamicSlotSpawnTimeoutPolicy.default_secs > config.dynamicSlotSpawnTimeoutPolicy.max_secs
  ) {
    throw new V3BlueprintRuntimeConfigError(
      'failed to parse V3 workstation-config: dynamic-slot-spawn :default_secs outside :min_secs..:max_secs',
    );
  }
  if (config.contextPackDispatchPolicy.min_parallel > config.contextPackDispatchPolicy.max_parallel) {
    throw new V3BlueprintRuntimeConfigError(
      'failed to parse V3 workstation-config: context-pack-run-wave :min_parallel exceeds :max_parallel',
    );
  }
  if (
    config.contextPackDispatchPolicy.default_max_parallel < config.contextPackDispatchPolicy.min_parallel
    || config.contextPackDispatchPolicy.default_max_parallel > config.contextPackDispatchPolicy.max_parallel
  ) {
    throw new V3BlueprintRuntimeConfigError(
      'failed to parse V3 workstation-config: context-pack-run-wave :default_max_parallel outside :min_parallel..:max_parallel',
    );
  }
  if (config.slotTtlPolicy.min_secs > config.slotTtlPolicy.max_secs) {
    throw new V3BlueprintRuntimeConfigError(
      'failed to parse V3 workstation-config: ttl :min_secs exceeds :max_secs',
    );
  }
  if (config.slotTtlPolicy.default_extend_secs > config.slotTtlPolicy.max_extend_secs) {
    throw new V3BlueprintRuntimeConfigError(
      'failed to parse V3 workstation-config: ttl :default_extend_secs exceeds :max_extend_secs',
    );
  }
  if (config.slotTtlPolicy.max_extend_secs < config.slotTtlPolicy.min_secs) {
    throw new V3BlueprintRuntimeConfigError(
      'failed to parse V3 workstation-config: ttl :max_extend_secs is below :min_secs',
    );
  }
  if (config.slotTtlPolicy.max_extend_secs > config.slotTtlPolicy.max_secs) {
    throw new V3BlueprintRuntimeConfigError(
      'failed to parse V3 workstation-config: ttl :max_extend_secs exceeds :max_secs',
    );
  }

  return config;
}

function defaultWorkstationRuntimeConfig(source) {
  return new WorkstationRuntimeConfig({
    slotDefaultProfiles: defaultSlotProfiles(),
    slotTemplates: defaultSlotTemplates(),
    allowedCwdPrefixes: DEFAULT_ALLOWED_CWD_PREFIXES,
    timeoutPolicy: defaultTimeoutPolicy(),
    ccSwarmTimeoutPolicy: defaultCcSwarmTimeoutPolicy(),
    ptySendTimeoutPolicy: defaultPtySendTimeoutPolicy(),
    dynamicSlotSpawnTimeoutPolicy: defaultDynamicSlotSpawnTimeoutPolicy(),
    contextPackDispatchPolicy: defaultContextPackDispatchPolicy(),
    slotTtlPolicy: defaultSlotTtlPolicy(),
    source,
    diagnostics: source === 'fallback-defaults'
      ? ['using embedded JS workstation runtime defaults']
      : [],
  });
}

function defaultSlotTemplates() {
  return new Map([
    ['coder', {
      name: 'coder',
      role: 'coder',
      description: 'Dynamic coder slot (ephemeral)',
      default_model_profile: DEFAULT_MODEL_PROFILE,
      mcp_config: DEFAULT_SLOT_MCP_CONFIG,
      default_cwd: DEFAULT_SLOT_DEFAULT_CWD,
    }],
    ['researcher', {
      name: 'researcher',
      role: 'coder',
      description: 'Dynamic researcher slot (read-only analysis)',
      default_model_profile: DEFAULT_MODEL_PROFILE,
      mcp_config: DEFAULT_SLOT_MCP_CONFIG,
      default_cwd: DEFAULT_SLOT_DEFAULT_CWD,
    }],
    ['ops', {
      name: 'ops',
      role: 'operator',
      description: 'Dynamic ops slot (ephemeral)',
      default_model_profile: 'daily-sonnet',
      mcp_config: DEFAULT_SLOT_MCP_CONFIG,
      default_cwd: DEFAULT_SLOT_DEFAULT_CWD,
    }],
  ]);
}

function defaultSlotProfiles() {
  return new Map([
    ['coder', DEFAULT_MODEL_PROFILE],
    ['researcher', DEFAULT_MODEL_PROFILE],
    ['ops', 'daily-sonnet'],
  ]);
}

function defaultTimeoutPolicy() {
  return {
    default_secs: DEFAULT_TIMEOUT_SECS,
    min_secs: MIN_TIMEOUT_SECS,
    max_secs: MAX_TIMEOUT_SECS,
    watchdog_grace_secs: WATCHDOG_GRACE_SECS,
    missing_session_probe_secs: MISSING_SESSION_PROBE_SECS,
  };
}

function defaultCcSwarmTimeoutPolicy() {
  return {
    default_secs: DEFAULT_CC_SWARM_TIMEOUT_SECS,
    min_secs: MIN_CC_SWARM_TIMEOUT_SECS,
    max_secs: MAX_CC_SWARM_TIMEOUT_SECS,
  };
}

function defaultPtySendTimeoutPolicy() {
  return {
    default_secs: DEFAULT_PTY_SEND_TIMEOUT_SECS,
    min_secs: MIN_PTY_SEND_TIMEOUT_SECS,
    max_secs: MAX_PTY_SEND_TIMEOUT_SECS,
  };
}

function defaultDynamicSlotSpawnTimeoutPolicy() {
  return {
    default_secs: DEFAULT_DYNAMIC_SLOT_SPAWN_TIMEOUT_SECS,
    min_secs: MIN_DYNAMIC_SLOT_SPAWN_TIMEOUT_SECS,
    max_secs: MAX_DYNAMIC_SLOT_SPAWN_TIMEOUT_SECS,
  };
}

function defaultContextPackDispatchPolicy() {
  return {
    default_max_parallel: DEFAULT_CONTEXT_PACK_MAX_PARALLEL,
    min_parallel: MIN_CONTEXT_PACK_MAX_PARALLEL,
    max_parallel: MAX_CONTEXT_PACK_MAX_PARALLEL,
  };
}

function defaultSlotTtlPolicy() {
  return {
    default_secs: DEFAULT_SLOT_TTL_SECS,
    min_secs: MIN_SLOT_TTL_SECS,
    max_secs: MAX_SLOT_TTL_SECS,
    default_extend_secs: DEFAULT_SLOT_EXTEND_SECS,
    max_extend_secs: MAX_SLOT_EXTEND_SECS,
  };
}

function requiredText(props, key, context) {
  const value = keywordPropText(props, key);
  if (!value) {
    throw new V3BlueprintRuntimeConfigError(`${context}: missing ${key}`);
  }
  return value;
}

function keywordPropList(props, key) {
  return nodeToStringArray(props[key]?.value);
}

function findFirstListByHead(nodes, targetHead) {
  for (const node of nodes) {
    if (isList(node) && head(node) === targetHead) return node;
    if (isList(node)) {
      const found = findFirstListByHead(node.children, targetHead);
      if (found) return found;
    }
  }
  return null;
}

function readPositiveInt(props, key, file) {
  const raw = keywordPropText(props, key);
  if (!raw || !/^[1-9][0-9]*$/.test(raw)) {
    throw new V3BlueprintRuntimeConfigError(
      `failed to parse V3 workstation-config ${file}: ${key} must be a positive integer`,
    );
  }
  return Number.parseInt(raw, 10);
}

function validateProfile(profile, label) {
  if (!PROFILE_RE.test(profile)) {
    throw new V3BlueprintRuntimeConfigError(
      `${label} default model profile must be a safe token, got ${JSON.stringify(profile)}`,
    );
  }
}
