import { runLispc } from './ocaml_lispc.mjs';

export const DEFAULT_V3_BLUEPRINT = '.missiond/v3/missiond-blueprint.lisp';

export function loadCompiledV3Contract({
  repoRoot = process.cwd(),
  blueprint = DEFAULT_V3_BLUEPRINT,
  semanticIr = true,
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

  const v3Surfaces = normalizeSurfaces(v3.compiled?.payload?.surfaces ?? []);
  const v3Functions = normalizeFunctions(v3.compiled?.payload?.functions ?? []);
  const facts = semantic?.compiled?.payload?.facts ?? [];
  const semanticSurfaces = normalizeSurfaces(
    facts.filter((fact) => fact?.kind === 'surface'),
  );
  const semanticFunctions = normalizeFunctions(
    facts.filter((fact) => fact?.kind === 'function'),
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
    surfaces,
    functions,
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
    surfaces: [],
    functions: [],
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
