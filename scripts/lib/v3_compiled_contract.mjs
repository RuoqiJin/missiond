import { runLispc } from './ocaml_lispc.mjs';

export const DEFAULT_V3_BLUEPRINT = '.missiond/v3/missiond-blueprint.lisp';

export function loadCompiledV3Contract({
  repoRoot = process.cwd(),
  blueprint = DEFAULT_V3_BLUEPRINT,
  semanticIr = true,
  workflows = false,
  workflowDir = '.missiond/workflows',
  timeoutMs = 60_000,
} = {}) {
  const diagnostics = [];
  const v3 = runLispc(['emit-v3', '--blueprint', blueprint], { repoRoot, timeoutMs });
  if (v3.ok !== true || !v3.compiled) {
    diagnostics.push(...lispcDiagnostics(v3, 'emit-v3'));
    return emptyContract({ ok: false, diagnostics, v3 });
  }

  let semantic = null;
  if (semanticIr) {
    semantic = runLispc(['emit-semantic-ir', '--blueprint', blueprint], { repoRoot, timeoutMs });
    if (semantic.ok !== true || !semantic.compiled) {
      diagnostics.push(...lispcDiagnostics(semantic, 'emit-semantic-ir'));
    }
  }
  let workflowProjection = null;
  if (workflows) {
    workflowProjection = runLispc(['emit-workflows', '--workflow-dir', workflowDir], { repoRoot, timeoutMs });
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
    workflowContracts,
    sourceUnits,
    workstationConfig,
    sourceHash: v3.compiled.source_hash,
    semanticSourceHash: semantic?.compiled?.source_hash ?? null,
  };
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
