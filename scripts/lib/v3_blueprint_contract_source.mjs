import fs from 'node:fs';
import path from 'node:path';

const BLUEPRINT_NOTES_SIDECAR = '.missiond/v3/evidence/blueprint-notes.lisp';

export function readBlueprintWithEvidenceSidecars(repoRoot, relPath) {
  const blueprintPath = path.join(repoRoot, relPath);
  const blueprint = readBlueprintResolvedSource(repoRoot, relPath);
  const sidecarPath = path.join(repoRoot, BLUEPRINT_NOTES_SIDECAR);
  if (!fs.existsSync(sidecarPath)) {
    return blueprint;
  }
  const sidecar = fs.readFileSync(sidecarPath, 'utf8');
  return `${blueprint}\n\n;; evidence sidecar included for contract-anchor checks\n${sidecar}`;
}

export function readBlueprintResolvedSource(repoRoot, relPath) {
  const blueprintPath = path.join(repoRoot, relPath);
  const blueprintDir = path.dirname(blueprintPath);
  const blueprint = fs.readFileSync(blueprintPath, 'utf8');
  return blueprint.replace(
    /\n([ \t]*)\(include\s+"(shards\/[^"]+)"\)/g,
    (match, indent, includePath) => {
      validateIncludePath(includePath);
      const shardPath = path.join(blueprintDir, includePath);
      const shard = fs.readFileSync(shardPath, 'utf8').trimEnd();
      return `\n${indent};; compiler-active include: ${includePath}\n${indent}${shard.replace(/\n/g, `\n${indent}`)}`;
    },
  );
}

function validateIncludePath(includePath) {
  if (!includePath.startsWith('shards/')) {
    throw new Error(`invalid blueprint include path: ${includePath}`);
  }
  if (path.isAbsolute(includePath) || includePath.split('/').includes('..')) {
    throw new Error(`unsafe blueprint include path: ${includePath}`);
  }
}
