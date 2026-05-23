export const BASELINE_AGENT_ENTRY_IDS = Object.freeze([
  'modify-board-backend',
  'modify-board-frontend',
  'modify-plan-execution',
  'modify-workstation-autopilot',
  'modify-mcp-tool',
  'modify-memory-provider',
  'modify-semantic-ir-ssot',
  'modify-workflow-delegation',
]);

export const PRIMARY_TOOL_FAMILIES = Object.freeze([
  'mission_board',
  'mission_workflow',
  'mission_workstation',
  'mission_context',
  'mission_memory',
  'mission_universe',
  'mission_ops',
  'mission_router',
  'mission_tool_directory',
]);

export function buildAgentSlices({ semanticJson, behaviorNavigationJson = null } = {}) {
  const facts = arrayOrEmpty(semanticJson?.payload?.facts);
  const sourceUnits = arrayOrEmpty(semanticJson?.payload?.source_units);
  const sourceDomains = arrayOrEmpty(semanticJson?.payload?.source_domains);
  const diagnostics = [...arrayOrEmpty(semanticJson?.diagnostics)];
  const surfaces = mapById(facts.filter((fact) => fact?.kind === 'surface'));
  const functions = mapById(facts.filter((fact) => fact?.kind === 'function'));
  const artifacts = mapById(facts.filter((fact) => fact?.kind === 'artifact_contract'));
  const runtimePolicies = mapById(facts.filter((fact) => fact?.kind === 'runtime_policy'));
  const checkerCommands = new Set(
    facts
      .filter((fact) => fact?.kind === 'checker_registry')
      .flatMap((fact) => arrayOrEmpty(fact.checks)),
  );
  const behaviorAnchors = arrayOrEmpty(behaviorNavigationJson?.payload?.anchors);
  const rawEntries = facts
    .filter((fact) => fact?.kind === 'agent_entry')
    .sort((a, b) => String(a.id).localeCompare(String(b.id)));

  for (const id of BASELINE_AGENT_ENTRY_IDS) {
    if (!rawEntries.some((entry) => entry.id === id)) {
      diagnostics.push(diag('agent-entry-index', `missing baseline agent entry ${id}`));
    }
  }

  const entries = rawEntries.map((entry) => enrichAgentEntry(entry, {
    surfaces,
    functions,
    artifacts,
    runtimePolicies,
    checkerCommands,
    behaviorAnchors,
    diagnostics,
  }));

  return {
    schema_version: 'missiond.compiled-agent-slices.v1',
    source_hash: semanticJson?.source_hash ?? null,
    generated_at: null,
    diagnostics,
    payload: {
      slice_policy: 'agents receive task-specific entry cards joined from Lisp-authored intent anchors and compiled semantic facts before full Lisp',
      entries,
      facts,
      source_units: sourceUnits,
      source_domains: sourceDomains,
    },
  };
}

export function validateCompiledAgentSlices(compiled) {
  const diagnostics = [];
  if (compiled?.schema_version !== 'missiond.compiled-agent-slices.v1') {
    diagnostics.push(diag('compiled-agent-slices.json', 'schema_version must be missiond.compiled-agent-slices.v1'));
  }
  const payload = compiled?.payload ?? {};
  const entries = arrayOrEmpty(payload.entries);
  const facts = arrayOrEmpty(payload.facts);
  if (entries.length === 0) diagnostics.push(diag('compiled-agent-slices.json', 'payload.entries must be non-empty'));
  if (facts.length === 0) diagnostics.push(diag('compiled-agent-slices.json', 'payload.facts must be retained for compatibility'));
  for (const id of BASELINE_AGENT_ENTRY_IDS) {
    if (!entries.some((entry) => entry.id === id)) {
      diagnostics.push(diag('compiled-agent-slices.json', `payload.entries missing baseline ${id}`));
    }
  }
  return diagnostics;
}

function enrichAgentEntry(entry, context) {
  const surfaceIds = stringArray(entry.surfaces);
  const functionIds = stringArray(entry.functions);
  const artifactIds = stringArray(entry.artifact_contracts);
  const runtimePolicyIds = stringArray(entry.runtime_policies);
  const checks = stringArray(entry.checks);
  const primaryFamily = stringOrNull(entry.primary_family);
  if (!PRIMARY_TOOL_FAMILIES.includes(primaryFamily)) {
    context.diagnostics.push(diag(sourceFile(entry), `agent entry ${entry.id} has unknown primary_family ${primaryFamily ?? '<missing>'}`));
  }
  for (const id of surfaceIds) {
    if (!context.surfaces.has(id)) context.diagnostics.push(diag(sourceFile(entry), `agent entry ${entry.id} references unknown surface ${id}`));
  }
  for (const id of functionIds) {
    if (!context.functions.has(id)) context.diagnostics.push(diag(sourceFile(entry), `agent entry ${entry.id} references unknown function ${id}`));
  }
  for (const id of artifactIds) {
    if (!context.artifacts.has(id)) context.diagnostics.push(diag(sourceFile(entry), `agent entry ${entry.id} references unknown artifact_contract ${id}`));
  }
  for (const id of runtimePolicyIds) {
    if (!context.runtimePolicies.has(id)) context.diagnostics.push(diag(sourceFile(entry), `agent entry ${entry.id} references unknown runtime_policy ${id}`));
  }
  for (const check of checks) {
    if (!checkExists(check, context.checkerCommands)) {
      context.diagnostics.push(diag(sourceFile(entry), `agent entry ${entry.id} references unknown check ${check}`));
    }
  }

  const surfaceFacts = surfaceIds.map((id) => context.surfaces.get(id)).filter(Boolean);
  const functionFacts = functionIds.map((id) => context.functions.get(id)).filter(Boolean);
  const readFirstOverride = stringArray(entry.read_first_override);
  const readFirst = unique(readFirstOverride.length > 0
    ? readFirstOverride
    : [
        ...surfaceFacts.flatMap((surface) => stringArray(surface.code)),
        ...sourceRefsFor([entry, ...surfaceFacts, ...functionFacts]).map((ref) => ref.file),
      ]).slice(0, 16);

  return {
    id: stringOrNull(entry.id),
    projectId: stringOrNull(entry.project_id) ?? 'missiond',
    label: stringOrNull(entry.label) ?? stringOrNull(entry.id),
    primaryFamily,
    intentKeywords: stringArray(entry.intent_keywords),
    surfaces: surfaceIds,
    functions: functionIds,
    artifactContracts: artifactIds,
    runtimePolicies: runtimePolicyIds,
    behaviorKinds: stringArray(entry.behavior_kinds),
    readFirst,
    writeScope: stringArray(entry.write_scope),
    mustNotTouch: stringArray(entry.must_not_touch),
    checks,
    fallback: stringOrNull(entry.fallback),
    authorityNotes: authorityNotes(entry, primaryFamily),
    sourceRefs: sourceRefsFor([entry, ...surfaceFacts, ...functionFacts]),
    behaviorAnchors: selectBehaviorAnchors(context.behaviorAnchors, stringArray(entry.behavior_kinds), readFirst),
  };
}

function authorityNotes(entry, primaryFamily) {
  return [
    'Navigation only; it does not grant write authority or bypass review gates.',
    `Use ${primaryFamily ?? 'mission_tool_directory'} as the primary MCP family before lower-level tools.`,
    'Keep generated compiled JSON machine-owned; update Lisp/source and rerun projection checks.',
    stringOrNull(entry.fallback) ? `Fallback: ${entry.fallback}` : null,
  ].filter(Boolean);
}

function selectBehaviorAnchors(anchors, kinds, readFirst) {
  if (kinds.length === 0) return [];
  const readFirstSet = new Set(readFirst);
  return anchors
    .filter((anchor) => kinds.includes(anchor?.kind) || kinds.includes(anchor?.role) || readFirstSet.has(anchor?.file))
    .slice(0, 12)
    .map((anchor) => ({
      id: stringOrNull(anchor.id),
      kind: stringOrNull(anchor.kind),
      role: stringOrNull(anchor.role),
      file: stringOrNull(anchor.file),
      line: Number.isInteger(anchor.line) ? anchor.line : null,
      symbol: stringOrNull(anchor.symbol),
    }));
}

function sourceRefsFor(facts) {
  return uniqueBy(
    facts
      .map((fact) => {
        const source = fact?.source ?? {};
        const file = stringOrNull(source.source_file);
        if (!file) return null;
        return {
          kind: stringOrNull(fact.kind),
          id: stringOrNull(fact.id),
          file,
          line: Number.isInteger(source.source_line) ? source.source_line : null,
          sourceHash: stringOrNull(source.source_hash),
        };
      })
      .filter(Boolean),
    (ref) => `${ref.kind}:${ref.id}:${ref.file}:${ref.line ?? ''}`,
  );
}

function checkExists(command, checkerCommands) {
  if (checkerCommands.has(command)) return true;
  const commandHead = command.split(/\s+/).slice(0, 2).join(' ');
  for (const known of checkerCommands) {
    if (known === commandHead || known.startsWith(`${commandHead} `) || command.startsWith(`${known} `)) {
      return true;
    }
  }
  return command.startsWith('node scripts/') || command.startsWith('pnpm ') || command.startsWith('cargo ');
}

function mapById(rows) {
  return new Map(rows.map((row) => [row.id, row]).filter(([id]) => typeof id === 'string' && id.length > 0));
}

function arrayOrEmpty(value) {
  return Array.isArray(value) ? value : [];
}

function stringArray(value) {
  return arrayOrEmpty(value).filter((item) => typeof item === 'string' && item.length > 0);
}

function stringOrNull(value) {
  return typeof value === 'string' && value.length > 0 ? value : null;
}

function unique(values) {
  return [...new Set(values.filter(Boolean))];
}

function uniqueBy(values, keyFn) {
  const seen = new Set();
  const out = [];
  for (const value of values) {
    const key = keyFn(value);
    if (seen.has(key)) continue;
    seen.add(key);
    out.push(value);
  }
  return out;
}

function sourceFile(fact) {
  return stringOrNull(fact?.source?.source_file) ?? '.missiond/v3/shards/agent-navigation.lisp';
}

function diag(file, message) {
  return { file, line: 1, column: 1, code: 'V3_AGENT_ENTRY_SLICES', message };
}
