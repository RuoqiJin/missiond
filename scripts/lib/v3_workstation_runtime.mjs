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
export const WATCHDOG_GRACE_SECS = 120;
export const MISSING_SESSION_PROBE_SECS = 120;

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
    source = 'defaults',
  } = {}) {
    this.slotDefaultProfiles = new Map(slotDefaultProfiles);
    this.timeoutPolicy = { ...timeoutPolicy };
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
  if (config.timeoutPolicy.min_secs > config.timeoutPolicy.max_secs) {
    throw new V3BlueprintRuntimeConfigError(
      'failed to parse V3 workstation-config: :min_secs exceeds :max_secs',
    );
  }

  return config;
}

function defaultWorkstationRuntimeConfig(source) {
  return new WorkstationRuntimeConfig({
    slotDefaultProfiles: defaultSlotProfiles(),
    timeoutPolicy: defaultTimeoutPolicy(),
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
