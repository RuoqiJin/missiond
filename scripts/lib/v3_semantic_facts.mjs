import {
  compiledArtifactContractMap,
  compiledCheckerRegistryMap,
  compiledContractSplitMap,
  compiledControlPlaneDomainMap,
  compiledFunctionMap,
  compiledRuntimePolicyMap,
  compiledSourceUnitMap,
  compiledSurfaceMap,
  loadCompiledV3Contract,
} from './v3_compiled_contract.mjs';

export function loadV3SemanticFacts(options = {}) {
  const contract = loadCompiledV3Contract({
    semanticIr: true,
    ...options,
  });
  return {
    contract,
    surfaces: compiledSurfaceMap(contract),
    functions: compiledFunctionMap(contract),
    artifacts: compiledArtifactContractMap(contract),
    runtimePolicies: compiledRuntimePolicyMap(contract),
    checkerRegistry: compiledCheckerRegistryMap(contract),
    contractSplits: compiledContractSplitMap(contract),
    controlPlaneDomains: compiledControlPlaneDomainMap(contract),
    sourceUnits: compiledSourceUnitMap(contract),
  };
}

export function semanticFactDiagnostics(facts, file = '.missiond/v3/missiond-blueprint.lisp') {
  if (facts.contract.ok) return [];
  return (facts.contract.diagnostics ?? []).map((diagnostic) => ({
    file: diagnostic.file ?? file,
    line: diagnostic.line ?? 1,
    column: diagnostic.column ?? 1,
    code: diagnostic.code ?? 'V3_SEMANTIC_FACTS_UNAVAILABLE',
    message: diagnostic.message ?? JSON.stringify(diagnostic),
  }));
}
