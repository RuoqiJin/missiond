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
  requestArtifacts: 'crates/missiond-daemon/src/handlers/knowledge/request/request_artifacts.rs',
  requestRespond: 'crates/missiond-daemon/src/handlers/knowledge/request/respond.rs',
  requestRespondEvents: 'crates/missiond-daemon/src/handlers/knowledge/request/respond/events.rs',
  requestRespondMaterialization:
    'crates/missiond-daemon/src/handlers/knowledge/request/respond/materialization.rs',
  requestRespondRouting: 'crates/missiond-daemon/src/handlers/knowledge/request/respond/routing.rs',
  requestReviewPacket: 'crates/missiond-daemon/src/handlers/knowledge/request/review_packet.rs',
  requestTests: 'crates/missiond-daemon/src/handlers/knowledge/request/tests.rs',
  directiveHandler: 'crates/missiond-daemon/src/handlers/knowledge/directive.rs',
  directiveCompileAuthoring: 'crates/missiond-daemon/src/handlers/knowledge/directive/compile_authoring.rs',
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
  requireText(diagnostics, files.blueprint, sources.blueprint, '(surface mission_request');
  requireText(diagnostics, files.blueprint, sources.blueprint, ':status "code-aligned"');
  requireText(diagnostics, files.blueprint, sources.blueprint, 'crates/missiond-daemon/src/handlers/knowledge/request/respond.rs');
  requireText(diagnostics, files.blueprint, sources.blueprint, 'crates/missiond-daemon/src/handlers/knowledge/request/respond/events.rs');
  requireText(diagnostics, files.blueprint, sources.blueprint, 'crates/missiond-daemon/src/handlers/knowledge/request/respond/materialization.rs');
  requireText(diagnostics, files.blueprint, sources.blueprint, 'crates/missiond-daemon/src/handlers/knowledge/request/respond/routing.rs');
  requireText(diagnostics, files.blueprint, sources.blueprint, 'crates/missiond-daemon/src/handlers/knowledge/request/review_packet.rs');
  requireText(diagnostics, files.blueprint, sources.blueprint, 'crates/missiond-daemon/src/handlers/knowledge/request/tests.rs');

  const directiveSurface = `${sources.directiveHandler}\n${sources.directiveCompileAuthoring}`;
  const directiveSurfaceLabel = `${files.directiveHandler} + ${files.directiveCompileAuthoring}`;
  requireText(diagnostics, directiveSurfaceLabel, directiveSurface, 'fn enrich_persisted_directive_sexp');
  requireText(diagnostics, directiveSurfaceLabel, directiveSurface, 'payload["compiled_sexp_preview"] = json!(persisted_preview_sexp)');
  requireText(diagnostics, directiveSurfaceLabel, directiveSurface, 'payload["compiled_sexp"] = json!(persisted_compiled_sexp)');

  const requestSurface = `${sources.requestHandler}\n${sources.requestArtifacts}\n${sources.requestRespond}\n${sources.requestRespondEvents}\n${sources.requestRespondMaterialization}\n${sources.requestRespondRouting}\n${sources.requestReviewPacket}`;
  const requestSurfaceLabel = `${files.requestHandler} + ${files.requestArtifacts} + ${files.requestRespond} + ${files.requestRespondEvents} + ${files.requestRespondMaterialization} + ${files.requestRespondRouting} + ${files.requestReviewPacket}`;
  requireText(diagnostics, requestSurfaceLabel, requestSurface, 'fn enrich_intent_alignment_projection');
  requireText(diagnostics, requestSurfaceLabel, requestSurface, 'fn enrich_materialized_plan_lisp');
  requireText(diagnostics, requestSurfaceLabel, requestSurface, 'atomic_write_artifact(&paths.plan, &enriched_plan_text, true)');
  requireText(diagnostics, requestSurfaceLabel, requestSurface, 'respond_result.insert("plan_materialized"');
  requireText(diagnostics, files.requestRespond, sources.requestRespond, 'mod events;');
  requireText(diagnostics, files.requestRespond, sources.requestRespond, 'mod materialization;');
  requireText(diagnostics, files.requestRespond, sources.requestRespond, 'mod routing;');
  requireText(diagnostics, files.requestRespond, sources.requestRespond, 'use self::events::{');
  requireText(diagnostics, files.requestRespondEvents, sources.requestRespondEvents, 'pub(in crate::handlers::knowledge::request) fn build_review_event_lisp');
  requireText(diagnostics, files.requestRespondEvents, sources.requestRespondEvents, 'pub(in crate::handlers::knowledge::request) fn next_event_seq');
  requireText(diagnostics, files.requestRespondEvents, sources.requestRespondEvents, 'pub(in crate::handlers::knowledge::request) fn next_action_for');
  requireText(diagnostics, files.requestRespondEvents, sources.requestRespondEvents, 'EVENT_SCHEMA');
  requireText(diagnostics, files.requestRespondRouting, sources.requestRespondRouting, 'pub(in crate::handlers::knowledge::request) enum RespondDecision');
  requireText(diagnostics, files.requestRespondRouting, sources.requestRespondRouting, 'pub(in crate::handlers::knowledge::request) fn parse_respond_decision');
  requireText(diagnostics, files.requestRespondRouting, sources.requestRespondRouting, 'pub(in crate::handlers::knowledge::request) fn resolve_directive_ref');
  requireText(diagnostics, files.requestRespondRouting, sources.requestRespondRouting, 'pub(in crate::handlers::knowledge::request) fn resolve_plan_ref');
  requireText(diagnostics, files.requestRespondRouting, sources.requestRespondRouting, 'pub(in crate::handlers::knowledge::request) fn build_respond_plan_compile_args');
  requireText(diagnostics, files.requestRespondMaterialization, sources.requestRespondMaterialization, 'pub(in crate::handlers::knowledge::request) async fn ensure_request_board_task');
  requireText(diagnostics, files.requestRespondMaterialization, sources.requestRespondMaterialization, 'pub(in crate::handlers::knowledge::request) fn enrich_materialized_plan_lisp');
  requireText(diagnostics, files.requestRespondMaterialization, sources.requestRespondMaterialization, 'pub(in crate::handlers::knowledge::request) async fn materialize_request_plan');
  requireText(diagnostics, files.requestRespondMaterialization, sources.requestRespondMaterialization, 'PlanStatus::Draft');
  requireText(diagnostics, requestSurfaceLabel, requestSurface, 'pub(super) fn derive_review_packet');
  requireText(diagnostics, requestSurfaceLabel, requestSurface, 'pub(super) fn classify_review_state');
  requireText(diagnostics, requestSurfaceLabel, requestSurface, 'pub(super) fn latest_review_event_checkpoint');
  requireText(diagnostics, files.requestHandler, sources.requestHandler, 'mod tests;');
  requireText(diagnostics, files.requestTests, sources.requestTests, 'request_lisp_carries_v3_policy');
  requireText(diagnostics, files.requestTests, sources.requestTests, 'derive_review_packet_intent_only_state');
  requireText(diagnostics, files.requestTests, sources.requestTests, 'respond_plan_compile_args_strips_write_file_by_default');

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
      :response-rule "start/advance/status/respond expose request-local :artifact_paths"))
  (implementation-map
    (surface mission_request
      :status "code-aligned"
      :code ["crates/missiond-daemon/src/handlers/knowledge/request/respond.rs"
             "crates/missiond-daemon/src/handlers/knowledge/request/respond/events.rs"
             "crates/missiond-daemon/src/handlers/knowledge/request/respond/materialization.rs"
             "crates/missiond-daemon/src/handlers/knowledge/request/respond/routing.rs"
             "crates/missiond-daemon/src/handlers/knowledge/request/review_packet.rs"
             "crates/missiond-daemon/src/handlers/knowledge/request/tests.rs"]
      :note "fixture")))`);
  writeFixture(root, DEFAULT_FILES.directiveHandler, `
mod compile_authoring;`);
  writeFixture(root, DEFAULT_FILES.directiveCompileAuthoring, `
fn enrich_persisted_directive_sexp() {}
payload["compiled_sexp_preview"] = json!(persisted_preview_sexp);
payload["compiled_sexp"] = json!(persisted_compiled_sexp);`);
  writeFixture(root, DEFAULT_FILES.requestHandler, `
mod tests;`);
  writeFixture(root, DEFAULT_FILES.requestTests, `
fn request_lisp_carries_v3_policy() {}
fn derive_review_packet_intent_only_state() {}
fn respond_plan_compile_args_strips_write_file_by_default() {}`);
  writeFixture(root, DEFAULT_FILES.requestArtifacts, `
fn enrich_intent_alignment_projection() {}`);
  writeFixture(root, DEFAULT_FILES.requestRespond, `
mod events;
mod materialization;
mod routing;
use self::events::{build_review_event_lisp, next_action_for, next_event_seq};
respond_result.insert("plan_materialized", json!(true));`);
  writeFixture(root, DEFAULT_FILES.requestRespondEvents, `
pub(in crate::handlers::knowledge::request) fn build_review_event_lisp() { EVENT_SCHEMA; }
pub(in crate::handlers::knowledge::request) fn next_event_seq() {}
pub(in crate::handlers::knowledge::request) fn next_action_for() {}`);
  writeFixture(root, DEFAULT_FILES.requestRespondMaterialization, `
pub(in crate::handlers::knowledge::request) async fn ensure_request_board_task() {}
pub(in crate::handlers::knowledge::request) fn enrich_materialized_plan_lisp() {}
pub(in crate::handlers::knowledge::request) async fn materialize_request_plan() {
  PlanStatus::Draft;
  atomic_write_artifact(&paths.plan, &enriched_plan_text, true);
}`);
  writeFixture(root, DEFAULT_FILES.requestRespondRouting, `
pub(in crate::handlers::knowledge::request) enum RespondDecision {}
pub(in crate::handlers::knowledge::request) fn parse_respond_decision() {}
pub(in crate::handlers::knowledge::request) fn resolve_directive_ref() {}
pub(in crate::handlers::knowledge::request) fn resolve_plan_ref() {}
pub(in crate::handlers::knowledge::request) fn build_respond_plan_compile_args() {}`);
  writeFixture(root, DEFAULT_FILES.requestReviewPacket, `
pub(super) fn derive_review_packet() {}
pub(super) fn classify_review_state() {}
pub(super) fn latest_review_event_checkpoint() {}`);
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
