export const RUNTIME_DOMAIN_SPECS = Object.freeze([
  { id: 'workstation', payloadKey: 'workstation', file: 'compiled-runtime-workstation.json' },
  { id: 'flow', payloadKey: 'flow', file: 'compiled-runtime-flow.json' },
  { id: 'compute', payloadKey: 'compute', file: 'compiled-runtime-compute.json' },
  { id: 'minimax', payloadKey: 'minimax', file: 'compiled-runtime-minimax.json' },
  { id: 'router', payloadKey: 'router', file: 'compiled-runtime-router.json' },
  { id: 'cascade', payloadKey: 'cascade', file: 'compiled-runtime-cascade.json' },
  { id: 'project-registry', payloadKey: 'projectRegistry', file: 'compiled-runtime-project-registry.json' },
  { id: 'capability-governance', payloadKey: 'capabilityGovernance', file: 'compiled-runtime-capability-governance.json' },
  { id: 'memory-kb', payloadKey: 'memoryKb', file: 'compiled-runtime-memory-kb.json' },
  { id: 'conversation-ingestion', payloadKey: 'conversationIngestion', file: 'compiled-runtime-conversation-ingestion.json' },
  { id: 'autopilot', payloadKey: 'autopilot', file: 'compiled-runtime-autopilot.json' },
  { id: 'learning-engine', payloadKey: 'learningEngine', file: 'compiled-runtime-learning-engine.json' },
]);

export function runtimeDomainFiles() {
  return Object.fromEntries(RUNTIME_DOMAIN_SPECS.map((spec) => [spec.id, spec.file]));
}

export function runtimeDomainSourceHashes(sourceHash) {
  return Object.fromEntries(RUNTIME_DOMAIN_SPECS.map((spec) => [spec.id, sourceHash]));
}
