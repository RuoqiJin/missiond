import fs from 'node:fs';
import path from 'node:path';
import { runLispc } from './ocaml_lispc.mjs';

export const DEFAULT_V3_BLUEPRINT = '.missiond/v3/missiond-blueprint.lisp';
export const BLUEPRINT_NOTES_SIDECAR = '.missiond/v3/evidence/blueprint-notes.lisp';

export function loadResolvedV3Contract({
  repoRoot = process.cwd(),
  blueprint = DEFAULT_V3_BLUEPRINT,
  includeEvidenceSidecar = false,
  timeoutMs = 60_000,
} = {}) {
  const targetRoot = path.resolve(repoRoot);
  const lispcRepoRoot = resolveLispcToolchainRoot(targetRoot);
  const blueprintArg = absolutizeFrom(targetRoot, blueprint);
  const diagnostics = [];
  const resolved = runLispc(['emit-resolved-v3', '--blueprint', blueprintArg], { repoRoot: lispcRepoRoot, timeoutMs });
  if (resolved.ok !== true || !resolved.compiled) {
    diagnostics.push(...lispcDiagnostics(resolved, 'emit-resolved-v3'));
    return {
      ok: false,
      diagnostics,
      resolved: resolved?.compiled ?? null,
      resolvedSource: null,
      sourceUnits: [],
      sourceHash: null,
    };
  }

  let resolvedSource = stringOrNull(resolved.compiled?.payload?.resolved_source);
  if (!resolvedSource) {
    diagnostics.push(diag(blueprint, 'RESOLVED_V3_SOURCE_MISSING', 'emit-resolved-v3 did not project payload.resolved_source'));
  }

  if (includeEvidenceSidecar && resolvedSource) {
    const sidecarPath = path.join(targetRoot, BLUEPRINT_NOTES_SIDECAR);
    if (fs.existsSync(sidecarPath)) {
      const sidecar = fs.readFileSync(sidecarPath, 'utf8');
      resolvedSource = `${resolvedSource}\n\n;; evidence sidecar included for contract-anchor checks\n${sidecar}`;
    }
  }

  return {
    ok: diagnostics.length === 0,
    diagnostics,
    resolved: resolved.compiled,
    resolvedSource,
    sourceUnits: normalizeSourceUnits(resolved.compiled?.payload?.source_units ?? []),
    sourceHash: resolved.compiled.source_hash,
  };
}

export function assertResolvedAnchors({ resolvedSource, anchors, file = DEFAULT_V3_BLUEPRINT } = {}) {
  const text = resolvedSource ?? '';
  return (anchors ?? [])
    .filter((anchor) => typeof anchor === 'string' && !text.includes(anchor))
    .map((anchor) => ({
      file,
      line: 1,
      column: 1,
      code: 'RESOLVED_V3_ANCHOR_MISSING',
      message: `missing required anchor: ${anchor}`,
    }));
}

export function loadCompiledV3Contract({
  repoRoot = process.cwd(),
  blueprint = DEFAULT_V3_BLUEPRINT,
  semanticIr = true,
  workflows = false,
  workflowDir = '.missiond/workflows',
  timeoutMs = 60_000,
} = {}) {
  const targetRoot = path.resolve(repoRoot);
  const lispcRepoRoot = resolveLispcToolchainRoot(targetRoot);
  const blueprintArg = absolutizeFrom(targetRoot, blueprint);
  const workflowDirArg = absolutizeFrom(targetRoot, workflowDir);
  const diagnostics = [];
  const v3 = runLispc(['emit-v3', '--blueprint', blueprintArg], { repoRoot: lispcRepoRoot, timeoutMs });
  if (v3.ok !== true || !v3.compiled) {
    diagnostics.push(...lispcDiagnostics(v3, 'emit-v3'));
    return emptyContract({ ok: false, diagnostics, v3 });
  }

  let semantic = null;
  if (semanticIr) {
    semantic = runLispc(['emit-semantic-ir', '--blueprint', blueprintArg], { repoRoot: lispcRepoRoot, timeoutMs });
    if (semantic.ok !== true || !semantic.compiled) {
      diagnostics.push(...lispcDiagnostics(semantic, 'emit-semantic-ir'));
    }
  }
  let workflowProjection = null;
  if (workflows) {
    workflowProjection = runLispc(['emit-workflows', '--workflow-dir', workflowDirArg], { repoRoot: lispcRepoRoot, timeoutMs });
    if (workflowProjection.ok !== true || !workflowProjection.compiled) {
      diagnostics.push(...lispcDiagnostics(workflowProjection, 'emit-workflows'));
    }
  }

  const v3Surfaces = normalizeSurfaces(v3.compiled?.payload?.surfaces ?? []);
  const v3Functions = normalizeFunctions(v3.compiled?.payload?.functions ?? []);
  const facts = semantic?.compiled?.payload?.facts ?? [];
  const semanticSurfaces = normalizeSurfaces(
    facts.filter((fact) => fact?.kind === 'surface'),
  );
  const semanticFunctions = normalizeFunctions(
    facts.filter((fact) => fact?.kind === 'function'),
  );
  const artifactContracts = normalizeArtifactContracts(
    facts.filter((fact) => fact?.kind === 'artifact_contract'),
  );
  const runtimePolicies = normalizeRuntimePolicies(
    facts.filter((fact) => fact?.kind === 'runtime_policy'),
  );
  const checkerRegistry = normalizeCheckerRegistry(
    facts.filter((fact) => fact?.kind === 'checker_registry'),
  );
  const contractSplits = normalizeContractSplits(
    facts.filter((fact) => fact?.kind === 'contract_split'),
  );
  const controlPlaneDomains = normalizeControlPlaneDomains(
    facts.filter((fact) => fact?.kind === 'control_plane_domain'),
  );
  const workflowContracts = normalizeWorkflowContracts([
    ...facts.filter((fact) => fact?.kind === 'workflow_contract'),
    ...(workflowProjection?.compiled?.payload?.workflows ?? []),
  ]);
  const sourceUnits = normalizeSourceUnits([
    ...(semantic?.compiled?.payload?.source_units ?? []),
    ...facts.filter((fact) => fact?.kind === 'module_source_unit'),
  ]);
  const workstationConfig = normalizeWorkstationConfig(
    facts.find((fact) => fact?.kind === 'workstation_config'),
  );
  const surfaces = semanticSurfaces.length > 0 ? semanticSurfaces : v3Surfaces;
  const functions = semanticFunctions.length > 0 ? semanticFunctions : v3Functions;

  const v3Ids = new Set(v3Surfaces.map((surface) => surface.id).filter(Boolean));
  const semanticIds = new Set(semanticSurfaces.map((surface) => surface.id).filter(Boolean));
  if (semanticIr && semanticSurfaces.length > 0) {
    for (const id of [...v3Ids].sort()) {
      if (!semanticIds.has(id)) {
        diagnostics.push(diag(blueprint, 'SEMANTIC_IR_SURFACE_MISSING', `semantic IR missing surface fact ${id}`));
      }
    }
    for (const id of [...semanticIds].sort()) {
      if (!v3Ids.has(id)) {
        diagnostics.push(diag(blueprint, 'SEMANTIC_IR_SURFACE_EXTRA', `semantic IR has surface fact not present in emit-v3 surfaces: ${id}`));
      }
    }
  }

  return {
    ok: diagnostics.length === 0,
    diagnostics,
    v3: v3.compiled,
    semantic: semantic?.compiled ?? null,
    workflowProjection: workflowProjection?.compiled ?? null,
    surfaces,
    functions,
    artifactContracts,
    runtimePolicies,
    checkerRegistry,
    contractSplits,
    controlPlaneDomains,
    workflowContracts,
    sourceUnits,
    workstationConfig,
    sourceHash: v3.compiled.source_hash,
    semanticSourceHash: semantic?.compiled?.source_hash ?? null,
  };
}

function absolutizeFrom(root, maybePath) {
  return path.isAbsolute(maybePath) ? maybePath : path.join(root, maybePath);
}

function resolveLispcToolchainRoot(targetRoot) {
  const direct = findLispcRoot(targetRoot);
  if (direct) return direct;
  const cwd = findLispcRoot(process.cwd());
  if (cwd) return cwd;
  return targetRoot;
}

function findLispcRoot(start) {
  let current = path.resolve(start);
  while (true) {
    if (fs.existsSync(path.join(current, 'tools', 'missiond_lispc', 'dune-project'))) {
      return current;
    }
    const parent = path.dirname(current);
    if (parent === current) return null;
    current = parent;
  }
}

export function compiledSurfaceIds(contract) {
  return [...new Set((contract?.surfaces ?? []).map((surface) => surface.id).filter(Boolean))].sort();
}

export function compiledSurfaceMap(contract) {
  return new Map(
    (contract?.surfaces ?? [])
      .filter((surface) => surface.id)
      .map((surface) => [surface.id, surface]),
  );
}

export function compiledFunctionMap(contract) {
  return new Map(
    (contract?.functions ?? [])
      .filter((fn) => fn.id)
      .map((fn) => [fn.id, fn]),
  );
}

export function compiledArtifactContractMap(contract) {
  return new Map(
    (contract?.artifactContracts ?? [])
      .filter((artifact) => artifact.id)
      .map((artifact) => [artifact.id, artifact]),
  );
}

export function compiledRuntimePolicyMap(contract) {
  return new Map(
    (contract?.runtimePolicies ?? [])
      .filter((policy) => policy.id)
      .map((policy) => [policy.id, policy]),
  );
}

export function compiledCheckerRegistryMap(contract) {
  return new Map(
    (contract?.checkerRegistry ?? [])
      .filter((entry) => entry.id)
      .map((entry) => [entry.id, entry]),
  );
}

export function compiledContractSplitMap(contract) {
  return new Map(
    (contract?.contractSplits ?? [])
      .filter((split) => split.id)
      .map((split) => [`${split.surface}:${split.id}`, split]),
  );
}

export function compiledControlPlaneDomainMap(contract) {
  return new Map(
    (contract?.controlPlaneDomains ?? [])
      .filter((domain) => domain.id)
      .map((domain) => [domain.id, domain]),
  );
}

export function compiledWorkflowMap(contract) {
  return new Map(
    (contract?.workflowContracts ?? [])
      .filter((workflow) => workflow.id)
      .map((workflow) => [workflow.id, workflow]),
  );
}

export function compiledSourceUnitMap(contract) {
  return new Map(
    (contract?.sourceUnits ?? [])
      .filter((unit) => unit.file)
      .map((unit) => [unit.file, unit]),
  );
}

function normalizeSurfaces(rows) {
  return rows
    .map((row) => ({
      id: stringOrNull(row?.id),
      status: normalizeStatus(row?.status),
      implements: stringArray(row?.implements),
      code: stringArray(row?.code),
      source: row?.source ?? null,
    }))
    .filter((row) => row.id);
}

function normalizeFunctions(rows) {
  return rows
    .map((row) => ({
      id: stringOrNull(row?.id),
      pillar: stringOrNull(row?.pillar),
      surface: stringOrNull(row?.surface),
      entry: stringArray(row?.entry),
      coreSteps: stringArray(row?.core_steps ?? row?.steps),
      egress: stringArray(row?.egress),
      source: row?.source ?? null,
    }))
    .filter((row) => row.id);
}

function normalizeArtifactContracts(rows) {
  return rows
    .map((row) => ({
      id: stringOrNull(row?.id),
      schema: stringOrNull(row?.schema),
      path: stringOrNull(row?.path),
      writer: stringOrNull(row?.writer),
      ssot: typeof row?.ssot === 'boolean' ? row.ssot : null,
      required: stringArray(row?.required),
      source: row?.source ?? null,
    }))
    .filter((row) => row.id);
}

function normalizeRuntimePolicies(rows) {
  return rows
    .map((row) => ({
      id: stringOrNull(row?.id),
      schemaVersion: stringOrNull(row?.schema_version ?? row?.schemaVersion),
      form: stringOrNull(row?.form),
      payloadKey: stringOrNull(row?.payload_key ?? row?.payloadKey),
      keywordKeys: stringArray(row?.keyword_keys ?? row?.keywordKeys),
      nestedForms: stringArray(row?.nested_forms ?? row?.nestedForms),
      source: row?.source ?? null,
    }))
    .filter((row) => row.id);
}

function normalizeCheckerRegistry(rows) {
  return rows
    .map((row) => ({
      id: stringOrNull(row?.id),
      checks: stringArray(row?.checks),
      source: row?.source ?? null,
    }))
    .filter((row) => row.id);
}

function normalizeContractSplits(rows) {
  return rows
    .map((row) => ({
      id: stringOrNull(row?.id),
      surface: stringOrNull(row?.surface),
      owns: stringArray(row?.owns),
      source: row?.source ?? null,
    }))
    .filter((row) => row.id && row.surface);
}

function normalizeControlPlaneDomains(rows) {
  return rows
    .map((row) => ({
      id: stringOrNull(row?.id),
      owner: stringOrNull(row?.owner),
      sourceRefs: stringArray(row?.source_refs ?? row?.sourceRefs),
      functions: stringArray(row?.functions),
      runtimeProjection: stringArray(row?.runtime_projection ?? row?.runtimeProjection),
      checker: stringArray(row?.checker),
      source: row?.source ?? null,
    }))
    .filter((row) => row.id);
}

function normalizeWorkflowContracts(rows) {
  return rows
    .map((row) => ({
      id: stringOrNull(row?.workflow_id ?? row?.id ?? row?.name),
      name: stringOrNull(row?.name),
      schema: stringOrNull(row?.schema),
      path: stringOrNull(row?.path),
      file: stringOrNull(row?.file),
      status: stringOrNull(row?.status),
      writer: stringOrNull(row?.writer),
      required: stringArray(row?.required),
      sourcePlans: stringArray(row?.source_plans ?? row?.sourcePlans),
      steps: stringArray(row?.steps),
      source: row?.source ?? null,
    }))
    .filter((row) => row.id);
}

function normalizeSourceUnits(rows) {
  const seen = new Set();
  const units = [];
  for (const row of rows) {
    const file = stringOrNull(row?.file ?? row?.id);
    if (!file || seen.has(file)) continue;
    seen.add(file);
    units.push({
      file,
      kind: stringOrNull(row?.kind === 'module_source_unit' ? row?.unit_kind : row?.kind),
      includedBy: stringOrNull(row?.included_by ?? row?.includedBy),
      includeLine: typeof row?.include_line === 'number' ? row.include_line : null,
      sourceHash: stringOrNull(row?.source_hash ?? row?.sourceHash),
    });
  }
  return units;
}

function normalizeWorkstationConfig(row) {
  if (!row) return null;
  return {
    id: stringOrNull(row?.id),
    modelProfiles: stringArray(row?.model_profiles ?? row?.modelProfiles),
    slotTemplates: stringArray(row?.slot_templates ?? row?.slotTemplates),
    source: row?.source ?? null,
  };
}

function stringOrNull(value) {
  if (typeof value !== 'string') return null;
  const trimmed = value.trim();
  return trimmed === '' ? null : trimmed;
}

function stringArray(value) {
  return Array.isArray(value)
    ? value.filter((item) => typeof item === 'string' && item.trim() !== '')
    : [];
}

function normalizeStatus(status) {
  return typeof status === 'string' ? status.replace(/^:/, '') : null;
}

function emptyContract({ ok, diagnostics, v3 }) {
  return {
    ok,
    diagnostics,
    v3: v3?.compiled ?? null,
    semantic: null,
    workflowProjection: null,
    surfaces: [],
    functions: [],
    artifactContracts: [],
    runtimePolicies: [],
    checkerRegistry: [],
    contractSplits: [],
    controlPlaneDomains: [],
    workflowContracts: [],
    sourceUnits: [],
    workstationConfig: null,
    sourceHash: null,
    semanticSourceHash: null,
  };
}

function lispcDiagnostics(result, command) {
  const diagnostics = result?.diagnostics ?? [];
  if (diagnostics.length > 0) {
    return diagnostics.map((d) => ({
      file: d.file ?? DEFAULT_V3_BLUEPRINT,
      line: d.line ?? 1,
      column: d.column ?? 1,
      code: d.code ?? 'COMPILED_V3_CONTRACT_FAILED',
      message: `[${command}] ${d.message ?? JSON.stringify(d)}`,
    }));
  }
  const message = result?.error
    ?? result?.stderr?.trim()
    ?? `${command} did not return a compiled payload`;
  return [diag(DEFAULT_V3_BLUEPRINT, 'COMPILED_V3_CONTRACT_FAILED', message)];
}

function diag(file, code, message) {
  return {
    file,
    line: 1,
    column: 1,
    code,
    message,
  };
}
