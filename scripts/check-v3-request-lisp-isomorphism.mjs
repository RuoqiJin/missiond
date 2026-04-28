#!/usr/bin/env node

import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';

const usage = `Usage:
  node scripts/check-v3-request-lisp-isomorphism.mjs [--json] [--dry-fixture]

Checks the narrow V3 mission_request Lisp/code isomorphism contract:
  - V3 blueprint declares request-local/compat directive ref materialization.
  - V3 blueprint declares request-local plan ref write-back after materialization.
  - daemon request/directive handlers implement the declared ref projection helpers.
  - MCP mission_request schema describes the same plan.lisp ref write-back contract.
`;

const DEFAULT_FILES = {
  blueprint: '.missiond/v3/missiond-blueprint.lisp',
  requestHandler: 'crates/missiond-daemon/src/handlers/knowledge/request.rs',
  directiveHandler: 'crates/missiond-daemon/src/handlers/knowledge/directive.rs',
  mcpRequest: 'crates/missiond-mcp/src/tools/knowledge/request.rs',
};

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

  const repoRoot = dryFixture ? buildFixture() : process.cwd();
  const diagnostics = checkFiles(repoRoot, DEFAULT_FILES);
  const result = {
    ok: diagnostics.length === 0,
    files: Object.keys(DEFAULT_FILES).length,
    diagnostics,
  };

  if (json) {
    console.log(JSON.stringify(result, null, 2));
  } else if (result.ok) {
    console.log('v3 request Lisp/code isomorphism check OK');
  } else {
    for (const d of diagnostics) {
      console.error(`${d.file}: ${d.message}`);
    }
    console.error(`v3 request Lisp/code isomorphism check FAILED — ${diagnostics.length} diagnostic(s)`);
  }

  process.exit(result.ok ? 0 : 1);
}

function checkFiles(root, files) {
  const diagnostics = [];
  const sources = {};
  for (const [key, rel] of Object.entries(files)) {
    const abs = path.join(root, rel);
    try {
      sources[key] = fs.readFileSync(abs, 'utf8');
    } catch (err) {
      diagnostics.push({ file: rel, message: `cannot read: ${err.message}` });
    }
  }
  if (diagnostics.length > 0) return diagnostics;

  requireText(diagnostics, files.blueprint, sources.blueprint, 'intent-alignment files MUST carry :directive_id + :version');
  requireText(diagnostics, files.blueprint, sources.blueprint, 'plan artifact MUST be amended with :plan_id + :version + :board_task_id');
  requireText(diagnostics, files.blueprint, sources.blueprint, 'start/advance/status/respond expose request-local :artifact_paths');

  requireText(diagnostics, files.directiveHandler, sources.directiveHandler, 'fn enrich_persisted_directive_sexp');
  requireText(diagnostics, files.directiveHandler, sources.directiveHandler, 'payload["compiled_sexp_preview"] = json!(persisted_preview_sexp)');
  requireText(diagnostics, files.directiveHandler, sources.directiveHandler, 'payload["compiled_sexp"] = json!(persisted_compiled_sexp)');

  requireText(diagnostics, files.requestHandler, sources.requestHandler, 'fn enrich_intent_alignment_projection');
  requireText(diagnostics, files.requestHandler, sources.requestHandler, 'fn enrich_materialized_plan_lisp');
  requireText(diagnostics, files.requestHandler, sources.requestHandler, 'atomic_write_artifact(&paths.plan, &enriched_plan_text, true)');
  requireText(diagnostics, files.requestHandler, sources.requestHandler, 'respond_result.insert("plan_materialized"');

  requireText(diagnostics, files.mcpRequest, sources.mcpRequest, ':plan_id/:version/:board_task_id');
  requireText(diagnostics, files.mcpRequest, sources.mcpRequest, 'writes the persisted ref back into plan.lisp');

  return diagnostics;
}

function requireText(diagnostics, file, source, needle) {
  if (!source.includes(needle)) {
    diagnostics.push({ file, message: `missing required contract text: ${needle}` });
  }
}

function buildFixture() {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'missiond-v3-request-isomorphism-'));
  writeFixture(root, DEFAULT_FILES.blueprint, `
(missiond-blueprint
  (artifact-contracts
    (artifact intent-alignment
      :materialization-rule "intent-alignment files MUST carry :directive_id + :version")
    (artifact plan
      :materialization-rule "plan artifact MUST be amended with :plan_id + :version + :board_task_id"))
  (unified-entry
    (review-packet
      :response-rule "start/advance/status/respond expose request-local :artifact_paths")))`);
  writeFixture(root, DEFAULT_FILES.directiveHandler, `
fn enrich_persisted_directive_sexp() {}
payload["compiled_sexp_preview"] = json!(persisted_preview_sexp);
payload["compiled_sexp"] = json!(persisted_compiled_sexp);`);
  writeFixture(root, DEFAULT_FILES.requestHandler, `
fn enrich_intent_alignment_projection() {}
fn enrich_materialized_plan_lisp() {}
atomic_write_artifact(&paths.plan, &enriched_plan_text, true);
respond_result.insert("plan_materialized", json!(true));`);
  writeFixture(root, DEFAULT_FILES.mcpRequest, `
"stamps :plan_id/:version/:board_task_id back into request-local plan.lisp"
"writes the persisted ref back into plan.lisp"`);
  return root;
}

function writeFixture(root, rel, text) {
  const abs = path.join(root, rel);
  fs.mkdirSync(path.dirname(abs), { recursive: true });
  fs.writeFileSync(abs, text.trimStart());
}

main();
