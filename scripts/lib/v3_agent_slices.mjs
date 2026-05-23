export const BASELINE_AGENT_ENTRY_IDS = Object.freeze([
  'modify-board-backend',
  'modify-board-frontend',
  'modify-plan-execution',
  'modify-workstation-autopilot',
  'modify-mcp-tool',
  'modify-memory-provider',
  'modify-semantic-ir-ssot',
  'modify-workflow-delegation',
  'modify-request-runtime',
  'modify-file-artifacts',
  'modify-review-gate',
  'modify-project-registry',
  'modify-router-policy',
  'modify-ops-infra',
  'modify-conversation-ingestion',
  'modify-source-hygiene-and-work-order',
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
  const policy = normalizeAgentNavigationPolicy(facts.find((fact) => fact?.kind === 'agent_navigation_policy'));
  const rawEntries = facts
    .filter((fact) => fact?.kind === 'agent_entry')
    .sort((a, b) => String(a.id).localeCompare(String(b.id)));

  if (!policy) {
    diagnostics.push(diag('agent-entry-index', 'missing agent_navigation_policy fact'));
  }
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
      policy,
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
  const policy = payload.policy ?? null;
  if (entries.length === 0) diagnostics.push(diag('compiled-agent-slices.json', 'payload.entries must be non-empty'));
  if (facts.length === 0) diagnostics.push(diag('compiled-agent-slices.json', 'payload.facts must be retained for compatibility'));
  if (!policy || typeof policy !== 'object') diagnostics.push(diag('compiled-agent-slices.json', 'payload.policy must be present'));
  for (const id of BASELINE_AGENT_ENTRY_IDS) {
    if (!entries.some((entry) => entry.id === id)) {
      diagnostics.push(diag('compiled-agent-slices.json', `payload.entries missing baseline ${id}`));
    }
  }
  return diagnostics;
}

export function buildProjectAgentNavigation({ semanticJson, universeJson, agentSlicesJson = null } = {}) {
  const diagnostics = [];
  const facts = arrayOrEmpty(semanticJson?.payload?.facts);
  const policy = normalizeAgentNavigationPolicy(facts.find((fact) => fact?.kind === 'agent_navigation_policy'));
  if (!policy) diagnostics.push(diag('compiled-project-agent-navigation.json', 'missing agent_navigation_policy fact'));
  const projects = arrayOrEmpty(universeJson?.payload?.projects)
    .filter((project) => typeof project?.id === 'string' && project.id.length > 0)
    .map((project) => projectNavigationCard(project, policy));
  const missiondEntries = arrayOrEmpty(agentSlicesJson?.payload?.entries);
  return {
    schema_version: 'missiond.compiled-project-agent-navigation.v1',
    source_hash: semanticJson?.source_hash ?? universeJson?.source_hash ?? null,
    generated_at: null,
    diagnostics,
    payload: {
      policy,
      projects,
      missiondEntries,
      source_units: arrayOrEmpty(universeJson?.payload?.source_units),
      source_domains: arrayOrEmpty(universeJson?.payload?.source_domains),
      coverage: {
        projectCount: projects.length,
        nativeSsotCount: projects.filter((project) => project.coverageState === 'native-ssot-present').length,
        metadataOnlyCount: projects.filter((project) => project.coverageState === 'metadata-only').length,
        missiondEntryCount: missiondEntries.length,
      },
    },
  };
}

export function validateCompiledProjectAgentNavigation(compiled) {
  const diagnostics = [];
  if (compiled?.schema_version !== 'missiond.compiled-project-agent-navigation.v1') {
    diagnostics.push(diag('compiled-project-agent-navigation.json', 'schema_version must be missiond.compiled-project-agent-navigation.v1'));
  }
  const projects = arrayOrEmpty(compiled?.payload?.projects);
  if (projects.length === 0) diagnostics.push(diag('compiled-project-agent-navigation.json', 'payload.projects must be non-empty'));
  if (!compiled?.payload?.policy) diagnostics.push(diag('compiled-project-agent-navigation.json', 'payload.policy must be present'));
  for (const project of projects) {
    if (!stringOrNull(project.projectId)) diagnostics.push(diag('compiled-project-agent-navigation.json', 'project card missing projectId'));
    if (!stringOrNull(project.coverageState)) diagnostics.push(diag('compiled-project-agent-navigation.json', `project ${project.projectId ?? '<missing>'} missing coverageState`));
    if (!Array.isArray(project.readFirst)) diagnostics.push(diag('compiled-project-agent-navigation.json', `project ${project.projectId ?? '<missing>'} readFirst must be an array`));
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

function normalizeAgentNavigationPolicy(fact) {
  if (!fact || typeof fact !== 'object') return null;
  return {
    id: stringOrNull(fact.id) ?? 'closure',
    schemaVersion: stringOrNull(fact.schema_version) ?? 'missiond.agent-navigation-policy.v1',
    reviewSidecar: stringOrNull(fact.review_sidecar) ?? '.missiond/v3/runtime/agent-navigation-review.json',
    projectNavigationArtifact: stringOrNull(fact.project_navigation_artifact) ?? '.missiond/v3/runtime/compiled/compiled-project-agent-navigation.json',
    qualityCorpus: stringOrNull(fact.quality_corpus) ?? '.missiond/v3/evidence/agent-navigation-quality-corpus.json',
    minFixturesPerEntry: numberOrDefault(fact.min_fixtures_per_entry, 2),
    minMatchAccuracy: numberOrDefault(fact.min_match_accuracy, 1),
    projectMode: stringOrNull(fact.project_mode) ?? 'read-only-registered-projects',
    actions: stringArray(fact.actions),
    checks: stringArray(fact.checks),
    rule: stringOrNull(fact.rule),
    source: fact.source ?? null,
  };
}

function projectNavigationCard(project, policy) {
  const projectId = stringOrNull(project.id);
  const projectRoot = stringOrNull(project.root);
  const localPaths = unique([
    stringOrNull(project.path),
    stringOrNull(project.intent),
    stringOrNull(project.backend),
    stringOrNull(project.frontend),
    stringOrNull(project.operations),
  ]);
  const readFirst = localPaths.map((item) => projectRoot && !item.startsWith('/') ? `${projectRoot}/${item}` : item);
  const coverageState = localPaths.length > 0 ? 'native-ssot-present' : 'metadata-only';
  return {
    id: `project:${projectId}`,
    projectId,
    projectRoot,
    label: `Project navigation: ${projectId}`,
    kind: stringOrNull(project.kind),
    surface: stringOrNull(project.surface),
    status: stringOrNull(project.status),
    coverageState,
    readFirst,
    checks: stringArray(project.checks),
    writeScope: [],
    mustNotTouch: ['sibling repositories are read-only from MissionD navigation closure', 'do not edit project-local SSOT unless an explicit project task owns it'],
    authorityNotes: [
      'Derived read-only from MissionD project registry.',
      `Project navigation mode: ${policy?.projectMode ?? 'read-only-registered-projects'}.`,
      coverageState === 'native-ssot-present'
        ? 'Project has registered SSOT paths; inspect them before proposing agent entries.'
        : 'Project has registry metadata only; suggest entries instead of mutating project files.',
    ],
    sourceRefs: [{
      kind: 'project',
      id: projectId,
      file: '.missiond/v3/shards/universe/project-registry.lisp',
      line: null,
      sourceHash: null,
    }],
    suggestedEntry: {
      schema: 'missiond.agent-entry-suggestion.v1',
      projectId,
      reason: coverageState === 'native-ssot-present'
        ? 'Project can support native agent-entry-index authoring in its own SSOT.'
        : 'Project registry has insufficient SSOT path metadata for native task-entry cards.',
      mustApplyInProjectRepo: true,
      missiondWillNotWriteSiblingRepo: true,
    },
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

function numberOrDefault(value, fallback) {
  const n = typeof value === 'number' ? value : Number(value);
  return Number.isFinite(n) ? n : fallback;
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
