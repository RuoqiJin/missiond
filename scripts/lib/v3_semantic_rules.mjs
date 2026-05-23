import crypto from 'node:crypto';

export function runSemanticRules({
  rules,
  repoRoot = process.cwd(),
  file = '.missiond/v3/missiond-blueprint.lisp',
  contract = null,
  gate = null,
  compiledTargets = [],
  runtimeBoundary = null,
  requiredLiveCheckIds = null,
} = {}) {
  const diagnostics = [];
  for (const rule of rules ?? []) {
    if (rule === 'required-final-live-checks') {
      diagnostics.push(...requiredFinalLiveChecks({ gate, file, requiredLiveCheckIds }));
    } else if (rule === 'source-domain-hash-consistency') {
      diagnostics.push(...sourceDomainHashConsistency({ compiledTargets, contract, file }));
    } else if (rule === 'compiled-surface-completeness') {
      diagnostics.push(...compiledSurfaceCompleteness({ contract, file }));
    } else if (rule === 'runtime-boundary-no-production-raw-lisp') {
      diagnostics.push(...runtimeBoundaryNoProductionRawLisp({ runtimeBoundary, repoRoot }));
    } else if (rule === 'generated-contracts-current') {
      diagnostics.push(...generatedContractsCurrent({ compiledTargets, file }));
    } else {
      diagnostics.push(diag(file, `unknown V3 semantic rule: ${rule}`, 'V3_SEMANTIC_RULE_UNKNOWN'));
    }
  }
  return diagnostics;
}

export function requiredFinalLiveChecks({ gate, file, requiredLiveCheckIds = null }) {
  const requiredIds = requiredLiveCheckIds ?? gate?.requiredLiveCheckIds ?? [];
  const liveCheckIds = new Set((gate?.liveChecks ?? []).map((check) => check.id).filter(Boolean));
  return requiredIds
    .filter((id) => !liveCheckIds.has(id))
    .map((id) => diag(file, `final convergence gate missing required live check ${id}`, 'V3_FINAL_GATE_REQUIRED_CHECK_MISSING'));
}

export function sourceDomainHashConsistency({ compiledTargets = [], contract = null, file }) {
  const diagnostics = [];
  const targets = [
    ...compiledTargets,
    ...(contract?.contractAbi ? [{ id: 'contract-abi', compiled: contract.contractAbi }] : []),
    ...(contract?.semantic ? [{ id: 'semantic-ir', compiled: contract.semantic }] : []),
    ...(contract?.v3 ? [{ id: 'v3', compiled: contract.v3 }] : []),
  ];
  for (const { id, compiled } of targets) {
    const domains = compiled?.payload?.source_domains;
    if (!Array.isArray(domains) || domains.length === 0) {
      diagnostics.push(diag(fileForTarget(id, file), `${id} compiled payload must include non-empty source_domains`, 'V3_SOURCE_DOMAINS_MISSING'));
      continue;
    }
    const seen = new Set();
    for (const domain of domains) {
      const domainId = typeof domain?.id === 'string' ? domain.id : '';
      if (!domainId) {
        diagnostics.push(diag(fileForTarget(id, file), `${id} source_domains contains an entry without id`, 'V3_SOURCE_DOMAIN_ID_MISSING'));
        continue;
      }
      if (seen.has(domainId)) {
        diagnostics.push(diag(fileForTarget(id, file), `${id} source_domains contains duplicate domain ${domainId}`, 'V3_SOURCE_DOMAIN_DUPLICATE'));
      }
      seen.add(domainId);
      const units = Array.isArray(domain?.source_units) ? domain.source_units : [];
      const expectedHash = typeof domain?.source_hash === 'string' ? domain.source_hash : '';
      const actualHash = md5Hex(units.map((unit) => String(unit?.source_hash ?? '')).join('\n'));
      if (expectedHash !== actualHash) {
        diagnostics.push(diag(fileForTarget(id, file), `${id} source domain ${domainId} hash mismatch from source_units: expected ${expectedHash || '<missing>'}, got ${actualHash}`, 'V3_SOURCE_DOMAIN_HASH_MISMATCH'));
      }
    }
  }
  return diagnostics;
}

export function compiledSurfaceCompleteness({ contract, file }) {
  const diagnostics = [];
  const surfaces = Array.isArray(contract?.surfaces) ? contract.surfaces : [];
  if (surfaces.length === 0) {
    diagnostics.push(diag(file, 'compiled contract has no implementation-map surfaces', 'V3_COMPILED_SURFACES_MISSING'));
    return diagnostics;
  }
  for (const surface of surfaces) {
    const id = surface?.id;
    if (!id) continue;
    const loc = sourceLoc(surface, file);
    if (surface.status === 'code-aligned-partial') {
      diagnostics.push(diag(loc.file, `surface "${id}" still carries :status "code-aligned-partial"; graduate it before merging`, 'V3_SURFACE_PARTIAL', loc));
      continue;
    }
    if (surface.status !== 'code-aligned') {
      diagnostics.push(diag(loc.file, `surface "${id}" must declare :status "code-aligned"; got ${JSON.stringify(surface.status)}`, 'V3_SURFACE_STATUS_INVALID', loc));
    }
    if (!Array.isArray(surface.code) || surface.code.length === 0) {
      diagnostics.push(diag(loc.file, `surface "${id}" must declare :code [...]`, 'V3_SURFACE_CODE_MISSING', loc));
    }
    if (typeof surface.note !== 'string' || surface.note.trim() === '') {
      diagnostics.push(diag(loc.file, `surface "${id}" must declare :note "..."`, 'V3_SURFACE_NOTE_MISSING', loc));
    }
  }
  return diagnostics;
}

export function runtimeBoundaryNoProductionRawLisp({ runtimeBoundary }) {
  const diagnostics = [];
  for (const item of runtimeBoundary?.required ?? []) {
    for (const needle of item.needles ?? []) {
      if (!item.source.includes(needle)) {
        diagnostics.push(diag(item.file, `missing production runtime boundary anchor: ${needle}`, 'PRODUCTION_RUNTIME_BOUNDARY_MISSING'));
      }
    }
  }
  for (const item of runtimeBoundary?.forbidden ?? []) {
    for (const needle of item.needles ?? []) {
      if (item.source.includes(needle)) {
        diagnostics.push(diag(item.file, `forbidden production runtime fallback/scanner anchor present: ${needle}`, 'PRODUCTION_RUNTIME_BOUNDARY_FORBIDDEN'));
      }
    }
  }
  return diagnostics;
}

export function generatedContractsCurrent({ compiledTargets = [], file }) {
  return compiledTargets
    .filter((target) => target?.ok === false)
    .map((target) => diag(target.file ?? file, target.message ?? `${target.id ?? 'generated contract'} is stale`, 'GENERATED_CONTRACT_STALE'));
}

export function sourceDomainBundleHash(sourceDomains) {
  return md5Hex((sourceDomains ?? []).map((domain) => String(domain?.source_hash ?? domain?.sourceHash ?? '')).join('\n'));
}

function sourceLoc(row, fallbackFile) {
  const source = row?.source ?? {};
  return {
    file: source.source_file ?? source.file ?? fallbackFile,
    line: source.source_line ?? source.line ?? 1,
    column: source.source_column ?? source.column ?? 1,
  };
}

function fileForTarget(id, fallbackFile) {
  if (id === 'runtime-config') return '.missiond/v3/runtime/compiled/compiled-runtime-config.json';
  if (id === 'universe') return '.missiond/v3/runtime/compiled/compiled-project-universe.json';
  if (id === 'semantic-ir') return '.missiond/v3/runtime/compiled/compiled-semantic-ir.json';
  if (id === 'contract-abi') return '.missiond/v3/runtime/compiled/compiled-contract-abi.json';
  if (id === 'v3') return '.missiond/v3/runtime/compiled/compiled-v3-blueprint.json';
  return fallbackFile;
}

function md5Hex(value) {
  return crypto.createHash('md5').update(String(value)).digest('hex');
}

function diag(file, message, code, loc = {}) {
  return {
    severity: 'error',
    file,
    line: loc.line ?? 1,
    column: loc.column ?? 1,
    code,
    message,
  };
}
