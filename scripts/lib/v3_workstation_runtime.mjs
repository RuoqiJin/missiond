import fs from 'node:fs';
import path from 'node:path';

import {
  head,
  isList,
  keywordPropText,
  nodeText,
  parseLisp,
  readKeywordProps,
} from './missiond_lisp.mjs';

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
export const WATCHDOG_GRACE_SECS = 120;
export const MISSING_SESSION_PROBE_SECS = 120;
export const DEFAULT_SLOT_TTL_SECS = 14400;
export const MIN_SLOT_TTL_SECS = 300;
export const MAX_SLOT_TTL_SECS = 28800;
export const DEFAULT_SLOT_EXTEND_SECS = 3600;
export const MAX_SLOT_EXTEND_SECS = 3600;

const PROFILE_RE = /^[A-Za-z0-9][A-Za-z0-9._-]*$/;

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
    timeoutPolicy = defaultTimeoutPolicy(),
    ccSwarmTimeoutPolicy = defaultCcSwarmTimeoutPolicy(),
    ptySendTimeoutPolicy = defaultPtySendTimeoutPolicy(),
    slotTtlPolicy = defaultSlotTtlPolicy(),
    source = 'defaults',
  } = {}) {
    this.slotDefaultProfiles = new Map(slotDefaultProfiles);
    this.timeoutPolicy = { ...timeoutPolicy };
    this.ccSwarmTimeoutPolicy = { ...ccSwarmTimeoutPolicy };
    this.ptySendTimeoutPolicy = { ...ptySendTimeoutPolicy };
    this.slotTtlPolicy = { ...slotTtlPolicy };
    this.source = source;
  }

  defaultModelProfileForTemplate(template) {
    return this.slotDefaultProfiles.get(template) ?? null;
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
  { blueprintPath = null, allowDefaultFallback = false } = {},
) {
  const repo = path.resolve(repoRoot);
  const explicitBlueprint = blueprintPath != null;
  const resolvedBlueprint = explicitBlueprint
    ? path.resolve(repo, blueprintPath)
    : path.join(repo, '.missiond', 'v3', 'missiond-blueprint.lisp');
  const missiondDir = path.join(repo, '.missiond');

  if (!fs.existsSync(resolvedBlueprint)) {
    if ((explicitBlueprint || fs.existsSync(missiondDir)) && !allowDefaultFallback) {
      throw new V3BlueprintRuntimeConfigError(`V3 blueprint missing at ${resolvedBlueprint}`);
    }
    return defaultWorkstationRuntimeConfig('fallback-defaults');
  }

  let source;
  try {
    source = fs.readFileSync(resolvedBlueprint, 'utf8');
  } catch (err) {
    throw new V3BlueprintRuntimeConfigError(
      `failed to read V3 blueprint ${resolvedBlueprint}: ${err.message}`,
    );
  }
  return parseWorkstationRuntimeConfig(source, resolvedBlueprint);
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
  for (const child of block.children) {
    if (!isList(child) || head(child) !== 'slot-template') continue;
    const template = nodeText(child.children[1]);
    if (!template) continue;
    const props = readKeywordProps(child, { start: 2 });
    const profile = keywordPropText(props, ':default-model-profile');
    if (profile) {
      validateProfile(profile, `${file}: slot-template ${template}`);
      config.slotDefaultProfiles.set(template, profile);
    }
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
    timeoutPolicy: defaultTimeoutPolicy(),
    ccSwarmTimeoutPolicy: defaultCcSwarmTimeoutPolicy(),
    ptySendTimeoutPolicy: defaultPtySendTimeoutPolicy(),
    slotTtlPolicy: defaultSlotTtlPolicy(),
    source,
  });
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

function defaultSlotTtlPolicy() {
  return {
    default_secs: DEFAULT_SLOT_TTL_SECS,
    min_secs: MIN_SLOT_TTL_SECS,
    max_secs: MAX_SLOT_TTL_SECS,
    default_extend_secs: DEFAULT_SLOT_EXTEND_SECS,
    max_extend_secs: MAX_SLOT_EXTEND_SECS,
  };
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
