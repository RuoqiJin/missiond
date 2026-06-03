import fs from 'node:fs';
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

const WORKSTATION_RUNTIME_SHARD = path.join(
  '.missiond',
  'v3',
  'shards',
  'workstation-runtime.lisp',
);

export function augmentWorkstationProviderLaunchPolicy(payload, { repo = process.cwd() } = {}) {
  const workstation = payload?.workstation ?? (
    payload?.domain === 'workstation' ? payload?.config : null
  );
  if (!workstation || !Array.isArray(workstation.workstation_pool)) {
    return payload;
  }
  const policies = loadWorkstationProviderLaunchPolicies(repo);
  if (policies.size === 0) {
    return payload;
  }
  for (const worker of workstation.workstation_pool) {
    const key = String(worker?.id ?? '');
    const slotKey = String(worker?.slot_id ?? '');
    const policy = policies.get(key) ?? policies.get(slotKey);
    if (!policy) continue;
    worker.skip_permissions = policy.skip_permissions;
    worker.provider_authorization_allowlist = [...policy.provider_authorization_allowlist];
  }
  return payload;
}

export function loadWorkstationProviderLaunchPolicies(repo = process.cwd()) {
  const sourcePath = path.join(repo, WORKSTATION_RUNTIME_SHARD);
  if (!fs.existsSync(sourcePath)) {
    return new Map();
  }
  const source = fs.readFileSync(sourcePath, 'utf8');
  const forms = parseLisp(source, WORKSTATION_RUNTIME_SHARD);
  const pool = findFirstListByHead(forms, 'workstation-pool');
  if (!pool) {
    return new Map();
  }
  const policies = new Map();
  for (const form of pool.children) {
    if (!isList(form) || head(form) !== 'worker') continue;
    const id = nodeText(form.children[1]);
    if (!id) continue;
    const props = readKeywordProps(form, { start: 2 });
    const skipPermissions = parseBool(
      keywordPropText(props, ':skip-permissions')
        ?? keywordPropText(props, ':skip_permissions'),
    ) ?? false;
    const allowlist = nodeToStringArray(
      props[':provider-authorization-allowlist']?.value
        ?? props[':provider_authorization_allowlist']?.value,
    );
    const policy = {
      skip_permissions: skipPermissions,
      provider_authorization_allowlist: allowlist,
    };
    policies.set(id, policy);
    const slotId = keywordPropText(props, ':slot-id') ?? keywordPropText(props, ':slot_id');
    if (slotId) {
      policies.set(slotId, policy);
    }
  }
  return policies;
}

function parseBool(value) {
  if (value == null) return null;
  const normalized = String(value).trim().toLowerCase();
  if (['true', 't', 'yes', 'on', '1'].includes(normalized)) return true;
  if (['false', 'nil', 'no', 'off', '0'].includes(normalized)) return false;
  return null;
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
