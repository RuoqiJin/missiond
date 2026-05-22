#!/usr/bin/env node

import fs from 'node:fs';
import path from 'node:path';
import {
  head,
  isList,
  nodeText,
  parseLisp,
  readKeywordProps,
  readLispFile,
} from './lib/missiond_lisp.mjs';
import { readBlueprintResolvedSource } from './lib/v3_blueprint_contract_source.mjs';

const usage = `Usage:
  node scripts/check-lisp-blueprint-compression.mjs [--json] [--dry-fixture]

Checks the Lisp compression boundary:
  - .missiond/v1/manifest.lisp parses and every indexed :path exists
  - .missiond/v3/missiond-blueprint.lisp parses
  - v3 contains the core executable sections:
      axioms, artifact-contracts, unified-entry, state-machines, policies,
      implementation-map, compression-contract
  - v3 declares the required artifacts and state machines
  - long explanatory notes live in .missiond/v3/evidence/blueprint-notes.lisp,
    keeping V3 as the executable SSOT rather than a prose archive
`;

const MAX_BLUEPRINT_NOTE_CHARS = 1100;
const BLUEPRINT_NOTES_SIDECAR = '.missiond/v3/evidence/blueprint-notes.lisp';

function main() {
  const args = process.argv.slice(2);
  let json = false;
  let dryFixture = false;
  for (const arg of args) {
    if (arg === '--help' || arg === '-h') {
      console.log(usage);
      process.exit(0);
    } else if (arg === '--json') {
      json = true;
    } else if (arg === '--dry-fixture') {
      dryFixture = true;
    } else {
      console.error(`unknown arg: ${arg}`);
      console.error(usage);
      process.exit(2);
    }
  }

  if (dryFixture) {
    runDryFixture();
    return;
  }

  const repoRoot = process.cwd();
  const diagnostics = [];
  validateV1Manifest(path.join(repoRoot, '.missiond/v1/manifest.lisp'), repoRoot, diagnostics);
  validateV3Blueprint(
    path.join(repoRoot, '.missiond/v3/missiond-blueprint.lisp'),
    repoRoot,
    diagnostics,
  );

  const result = {
    ok: diagnostics.length === 0,
    files: 2,
    diagnostics,
  };

  if (json) {
    console.log(JSON.stringify(result, null, 2));
  } else if (result.ok) {
    console.log('lisp-blueprint-compression check OK (v1 manifest + v3 blueprint)');
  } else {
    for (const d of diagnostics) {
      console.error(`${d.file}:${d.line ?? 1}:${d.column ?? 1}: ${d.message}`);
    }
    console.error(`lisp-blueprint-compression check FAILED — ${diagnostics.length} diagnostic(s)`);
  }

  process.exit(result.ok ? 0 : 1);
}

function validateV1Manifest(file, repoRoot, diagnostics) {
  const forms = safeRead(file, diagnostics);
  if (!forms) return;
  const root = singleRoot(file, forms, 'missiond-v1-lisp-manifest', diagnostics);
  if (!root) return;

  const fileNodes = findAll(root, 'file');
  if (fileNodes.length < 20) {
    diagnostics.push(diag(file, root, `expected at least 20 v1 file entries, found ${fileNodes.length}`));
  }

  const seen = new Set();
  for (const node of fileNodes) {
    const props = readKeywordProps(node, { start: 1 });
    const p = nodeText(props[':path']?.value);
    if (!p) {
      diagnostics.push(diag(file, node, 'v1 file entry missing :path'));
      continue;
    }
    if (seen.has(p)) {
      diagnostics.push(diag(file, node, `duplicate v1 path: ${p}`));
      continue;
    }
    seen.add(p);
    const abs = path.resolve(repoRoot, p);
    if (!fs.existsSync(abs)) {
      diagnostics.push(diag(file, node, `indexed v1 path does not exist: ${p}`));
    }
  }
}

function validateV3Blueprint(file, repoRoot, diagnostics) {
  let forms;
  try {
    forms = parseLisp(readBlueprintResolvedSource(repoRoot, path.relative(repoRoot, file)), file);
  } catch (err) {
    diagnostics.push({
      file,
      line: err.line ?? 1,
      column: err.column ?? 1,
      message: err.message,
    });
    return;
  }
  const root = singleRoot(file, forms, 'missiond-blueprint', diagnostics);
  if (!root) return;
  validateV3Root(file, root, diagnostics);
  validateNoteDensity(file, root, diagnostics);
  validateBlueprintNotesSidecar(path.join(repoRoot, BLUEPRINT_NOTES_SIDECAR), diagnostics);
}

function validateV3Root(file, root, diagnostics) {
  const requiredSections = [
    'axioms',
    'artifact-contracts',
    'unified-entry',
    'state-machines',
    'policies',
    'implementation-map',
    'compression-contract',
  ];
  for (const section of requiredSections) {
    if (!findImmediate(root, section)) {
      diagnostics.push(diag(file, root, `v3 blueprint missing (${section} ...) section`));
    }
  }

  const artifactSection = findImmediate(root, 'artifact-contracts');
  const artifactIds = new Set(
    findAll(artifactSection, 'artifact')
      .map((node) => nodeText(node.children[1]))
      .filter(Boolean),
  );
  for (const required of [
    'mission-request',
    'intent-alignment',
    'plan',
    'workflow',
    'lifecycle-event',
    'final-report',
    'verification-receipt',
  ]) {
    if (!artifactIds.has(required)) {
      diagnostics.push(diag(file, artifactSection ?? root, `v3 missing artifact ${required}`));
    }
  }

  const stateMachineIds = new Set(
    findAll(root, 'state-machine')
      .map((node) => nodeText(node.children[1]))
      .filter(Boolean),
  );
  for (const required of ['unified-entry', 'execution-lifecycle']) {
    if (!stateMachineIds.has(required)) {
      diagnostics.push(diag(file, root, `v3 missing state-machine ${required}`));
    }
  }

  const surfaceIds = new Set(
    findAll(root, 'surface')
      .map((node) => nodeText(node.children[1]))
      .filter(Boolean),
  );
  for (const required of ['mission_request', 'mission_directive', 'mission_plan', 'mission_workflow']) {
    if (!surfaceIds.has(required)) {
      diagnostics.push(diag(file, root, `v3 missing implementation surface ${required}`));
    }
  }
}

function validateNoteDensity(file, root, diagnostics) {
  for (const node of findKeywordValues(root, ':note')) {
    const text = nodeText(node);
    if (text && text.length > MAX_BLUEPRINT_NOTE_CHARS) {
      diagnostics.push(
        diag(
          file,
          node,
          `:note too long (${text.length} chars > ${MAX_BLUEPRINT_NOTE_CHARS}); move detail to ${BLUEPRINT_NOTES_SIDECAR}`,
        ),
      );
    }
  }
}

function validateBlueprintNotesSidecar(file, diagnostics) {
  const forms = safeRead(file, diagnostics);
  if (!forms) return;
  const root = singleRoot(file, forms, 'missiond-v3-blueprint-note-evidence', diagnostics);
  if (!root) return;
  const notes = findAll(root, 'note');
  if (notes.length < 10) {
    diagnostics.push(diag(file, root, `expected moved blueprint note evidence, found ${notes.length}`));
  }
  const ids = new Set();
  for (const note of notes) {
    const props = readKeywordProps(note, { start: 1 });
    const id = nodeText(props[':id']?.value);
    const text = nodeText(props[':text']?.value);
    if (!id) diagnostics.push(diag(file, note, 'sidecar note missing :id'));
    else if (ids.has(id)) diagnostics.push(diag(file, note, `duplicate sidecar note id: ${id}`));
    else ids.add(id);
    if (!text || text.length <= MAX_BLUEPRINT_NOTE_CHARS) {
      diagnostics.push(diag(file, note, 'sidecar note :text should hold the moved long explanation'));
    }
  }
}

function safeRead(file, diagnostics) {
  try {
    return readLispFile(file);
  } catch (err) {
    diagnostics.push({
      file,
      line: err.line ?? 1,
      column: err.column ?? 1,
      message: err.message,
    });
    return null;
  }
}

function singleRoot(file, forms, expectedHead, diagnostics) {
  const roots = forms.filter((form) => isList(form) && head(form) === expectedHead);
  if (roots.length !== 1) {
    diagnostics.push({
      file,
      line: 1,
      column: 1,
      message: `expected exactly one (${expectedHead} ...) root, found ${roots.length}`,
    });
    return null;
  }
  return roots[0];
}

function findImmediate(node, wantedHead) {
  if (!isList(node)) return null;
  return node.children.find((child) => isList(child) && head(child) === wantedHead) ?? null;
}

function findAll(node, wantedHead, out = []) {
  if (!node) return out;
  if (isList(node)) {
    if (head(node) === wantedHead) out.push(node);
    for (const child of node.children) findAll(child, wantedHead, out);
  }
  return out;
}

function findKeywordValues(node, keyword, out = []) {
  if (!isList(node)) return out;
  for (let i = 0; i < node.children.length - 1; i += 1) {
    if (nodeText(node.children[i]) === keyword) out.push(node.children[i + 1]);
  }
  for (const child of node.children) findKeywordValues(child, keyword, out);
  return out;
}

function diag(file, node, message) {
  return {
    file,
    line: node?.loc?.line ?? 1,
    column: node?.loc?.column ?? 1,
    message,
  };
}

function runDryFixture() {
  const good = parseLisp(`
    (missiond-blueprint
      (axioms)
      (artifact-contracts
        (artifact mission-request)
        (artifact intent-alignment)
        (artifact plan)
        (artifact workflow)
        (artifact lifecycle-event)
        (artifact final-report)
        (artifact verification-receipt))
      (unified-entry)
      (state-machines
        (state-machine unified-entry)
        (state-machine execution-lifecycle))
      (policies)
      (implementation-map
        (surface mission_request)
        (surface mission_directive)
        (surface mission_plan)
        (surface mission_workflow))
      (compression-contract))`);
  const diagnostics = [];
  validateV3BlueprintAst('<fixture-good>', good, diagnostics);
  if (diagnostics.length !== 0) {
    throw new Error(`good fixture failed: ${JSON.stringify(diagnostics)}`);
  }

  const bad = parseLisp(`
    (missiond-blueprint
      (artifact-contracts (artifact plan))
      (unified-entry)
      (state-machines (state-machine unified-entry))
      (policies)
      (implementation-map)
      (compression-contract))`);
  const badDiagnostics = [];
  validateV3BlueprintAst('<fixture-bad>', bad, badDiagnostics);
  if (badDiagnostics.length === 0) {
    throw new Error('bad fixture unexpectedly passed');
  }
  console.log('lisp-blueprint-compression dry-fixture OK (2 cases)');
}

function validateV3BlueprintAst(file, forms, diagnostics) {
  const root = singleRoot(file, forms, 'missiond-blueprint', diagnostics);
  if (!root) return;
  validateV3Root(file, root, diagnostics);
}

main();
