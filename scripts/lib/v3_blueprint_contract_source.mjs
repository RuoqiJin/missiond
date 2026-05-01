import fs from 'node:fs';
import path from 'node:path';

const BLUEPRINT_NOTES_SIDECAR = '.missiond/v3/evidence/blueprint-notes.lisp';

export function readBlueprintWithEvidenceSidecars(repoRoot, relPath) {
  const blueprintPath = path.join(repoRoot, relPath);
  const blueprint = fs.readFileSync(blueprintPath, 'utf8');
  const sidecarPath = path.join(repoRoot, BLUEPRINT_NOTES_SIDECAR);
  if (!fs.existsSync(sidecarPath)) {
    return blueprint;
  }
  const sidecar = fs.readFileSync(sidecarPath, 'utf8');
  return `${blueprint}\n\n;; evidence sidecar included for contract-anchor checks\n${sidecar}`;
}
