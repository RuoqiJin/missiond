import { loadResolvedV3Contract } from './v3_compiled_contract.mjs';

export function readBlueprintWithEvidenceSidecars(repoRoot, relPath) {
  return readResolvedBlueprint(repoRoot, relPath, { includeEvidenceSidecar: true });
}

export function readBlueprintResolvedSource(repoRoot, relPath) {
  return readResolvedBlueprint(repoRoot, relPath, { includeEvidenceSidecar: false });
}

function readResolvedBlueprint(repoRoot, blueprint, { includeEvidenceSidecar }) {
  const contract = loadResolvedV3Contract({
    repoRoot,
    blueprint,
    includeEvidenceSidecar,
  });
  if (!contract.ok || !contract.resolvedSource) {
    const detail = (contract.diagnostics ?? [])
      .map((diagnostic) => diagnostic.message ?? JSON.stringify(diagnostic))
      .join('; ');
    throw new Error(`missiond-lispc emit-resolved-v3 failed for ${blueprint}: ${detail || 'missing resolved_source'}`);
  }
  return contract.resolvedSource;
}
