#!/usr/bin/env node

import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';

const usage = `Usage:
  node scripts/check-v3-incident-governance-isomorphism.mjs [--json] [--dry-fixture]

Checks the V3 incident-governance Lisp/code isomorphism contract:
  - question.rs stays a thin facade for question/incident/trace/auth/stats tools.
  - question CRUD, decision stats, LLM trace, Gemini auth, and incident adapters are split.
  - legacy mission_question_* and mission_incident_* routes enter the same V3 facade.
`;

const DEFAULT_FILES = {
  blueprint: '.missiond/v3/missiond-blueprint.lisp',
  dispatcher: 'crates/missiond-daemon/src/handlers/mod.rs',
  facade: 'crates/missiond-daemon/src/handlers/comm/question.rs',
  questionFlow: 'crates/missiond-daemon/src/handlers/comm/question/question_flow.rs',
  decision: 'crates/missiond-daemon/src/handlers/comm/question/decision.rs',
  llmTrace: 'crates/missiond-daemon/src/handlers/comm/question/llm_trace.rs',
  auth: 'crates/missiond-daemon/src/handlers/comm/question/auth.rs',
  incident: 'crates/missiond-daemon/src/handlers/comm/question/incident.rs',
  sysinfraMisc: 'crates/missiond-daemon/src/handlers/sysinfra/misc.rs',
  mcp: 'crates/missiond-mcp/src/tools/comm/question.rs',
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
    console.log('v3 incident-governance Lisp/code isomorphism check OK');
  } else {
    for (const d of diagnostics) {
      console.error(`${d.file}: ${d.message}`);
    }
    console.error(
      `v3 incident-governance Lisp/code isomorphism check FAILED -- ${diagnostics.length} diagnostic(s)`,
    );
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

  requireAll(diagnostics, files.blueprint, sources.blueprint, [
    'incident-governance',
    '(surface incident-governance',
    ':status "code-aligned"',
    'crates/missiond-daemon/src/handlers/comm/question.rs',
    'crates/missiond-daemon/src/handlers/comm/question/question_flow.rs',
    'crates/missiond-daemon/src/handlers/comm/question/decision.rs',
    'crates/missiond-daemon/src/handlers/comm/question/llm_trace.rs',
    'crates/missiond-daemon/src/handlers/comm/question/auth.rs',
    'crates/missiond-daemon/src/handlers/comm/question/incident.rs',
    'crates/missiond-daemon/src/handlers/mod.rs',
    'crates/missiond-daemon/src/handlers/sysinfra/misc.rs',
    'crates/missiond-mcp/src/tools/comm/question.rs',
    'scripts/check-v3-incident-governance-isomorphism.mjs',
    'question.rs is the thin incident-governance facade',
    'question/question_flow.rs owns mission_question',
    'question/decision.rs owns mission_decision_stats',
    'question/llm_trace.rs owns mission_llm_trace',
    'question/auth.rs owns mission_gemini_auth',
    'question/incident.rs owns mission_incident routing',
    'sysinfra/misc.rs remains the legacy incident and Gemini execution adapter',
    'node scripts/check-v3-incident-governance-isomorphism.mjs',
  ]);

  requireAll(diagnostics, files.dispatcher, sources.dispatcher, [
    '"mission_question"',
    '"mission_decision_stats"',
    '"mission_incident"',
    '"mission_llm_trace"',
    '"mission_gemini_auth" => question::handle(state, name, args).await',
    'n if n.starts_with("mission_question_") => question::handle(state, n, args).await',
    'n if n.starts_with("mission_incident_") => question::handle(state, n, args).await',
    'n if n == "mission_health"',
  ]);

  requireAll(diagnostics, files.facade, sources.facade, [
    'mod auth;',
    'mod decision;',
    'mod incident;',
    'mod llm_trace;',
    'mod question_flow;',
    '"mission_question" => question_flow::handle_consolidated(state, args).await',
    '"mission_decision_stats" => decision::handle_stats(state, args).await',
    '"mission_llm_trace" => llm_trace::handle(state, args).await',
    '"mission_gemini_auth" => auth::handle(state, args).await',
    '"mission_incident" => incident::handle_consolidated(state, args).await',
    'n if n.starts_with("mission_incident_") => incident::handle_legacy(state, n, args).await',
    'Unknown question tool',
  ]);

  requireAll(diagnostics, files.questionFlow, sources.questionFlow, [
    'QuestionCreateArgs',
    'QuestionListArgs',
    'QuestionAnswerArgs',
    'QuestionIdArgs',
    'handle_consolidated',
    'handle_legacy',
    'handle_create',
    'handle_list',
    'handle_get',
    'handle_answer',
    'handle_dismiss',
    'list_running_autopilot_tasks',
    'CreateAgentQuestionInput',
    'create_agent_question',
    'list_agent_questions',
    'get_agent_question',
    'answer_agent_question',
    'dismiss_agent_question',
    'QuestionEvent::Created',
    'QuestionEvent::Resolved',
    'TaskEvent::Completed',
  ]);

  requireAll(diagnostics, files.decision, sources.decision, [
    'handle_stats',
    'decision_stats',
    'hours',
  ]);

  requireAll(diagnostics, files.llmTrace, sources.llmTrace, [
    'gemini_trace',
    'mission_gemini_trace',
    'gemini_stats',
    'mission_gemini_stats',
    'gemini_watch',
    'watch_action',
    'mission_gemini_watch',
    'gemini_auth',
    'mission_gemini_auth',
    'jarvis_logs',
    'mission_jarvis_logs',
    'jarvis_trace',
    'mission_jarvis_trace',
  ]);

  requireAll(diagnostics, files.auth, sources.auth, [
    'mission_gemini_auth',
    'crate::handlers::misc::handle',
  ]);

  requireAll(diagnostics, files.incident, sources.incident, [
    'handle_consolidated',
    'handle_legacy',
    'mission_incident_test',
    'mission_incident_list',
    'mission_incident_get',
    'mission_incident_remediate',
    'mission_incident_status',
    'mission_incident_close',
    'Unknown incident tool',
  ]);

  requireAll(diagnostics, files.sysinfraMisc, sources.sysinfraMisc, [
    'mission_incident_test',
    'mission_incident_list',
    'incident_get',
    'incident_remediate',
    'incident_status',
    'incident_close',
    'mission_gemini_auth',
    'publish_incident',
    'triage_incident',
    'is_safe_to_close_task',
  ]);

  requireAll(diagnostics, files.mcp, sources.mcp, [
    'ToolDefinition::new',
    '"mission_question"',
    '"mission_llm_trace"',
    '"mission_decision_stats"',
    '"mission_gemini_auth"',
    '"mission_incident"',
    '"create"',
    '"answer"',
    '"dismiss"',
    '"remediate"',
    '"close"',
  ]);

  return diagnostics;
}

function requireAll(diagnostics, file, source, needles) {
  for (const needle of needles) {
    if (!source.includes(needle)) {
      diagnostics.push({ file, message: `missing required contract text: ${needle}` });
    }
  }
}

function buildFixture() {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'missiond-v3-incident-governance-isomorphism-'));
  writeFixture(root, DEFAULT_FILES.blueprint, `
(missiond-blueprint
  (implementation-map
    (surface incident-governance
      :status "code-aligned"
      :code ["crates/missiond-daemon/src/handlers/comm/question.rs"
             "crates/missiond-daemon/src/handlers/comm/question/question_flow.rs"
             "crates/missiond-daemon/src/handlers/comm/question/decision.rs"
             "crates/missiond-daemon/src/handlers/comm/question/llm_trace.rs"
             "crates/missiond-daemon/src/handlers/comm/question/auth.rs"
             "crates/missiond-daemon/src/handlers/comm/question/incident.rs"
             "crates/missiond-daemon/src/handlers/mod.rs"
             "crates/missiond-daemon/src/handlers/sysinfra/misc.rs"
             "crates/missiond-mcp/src/tools/comm/question.rs"
             "scripts/check-v3-incident-governance-isomorphism.mjs"]
      :note "question.rs is the thin incident-governance facade; question/question_flow.rs owns mission_question; question/decision.rs owns mission_decision_stats; question/llm_trace.rs owns mission_llm_trace; question/auth.rs owns mission_gemini_auth; question/incident.rs owns mission_incident routing; sysinfra/misc.rs remains the legacy incident and Gemini execution adapter."))
  (compression-contract
    :checks ["node scripts/check-v3-incident-governance-isomorphism.mjs"]))`);

  writeFixture(root, DEFAULT_FILES.dispatcher, `
"mission_question" "mission_decision_stats" "mission_incident" "mission_llm_trace"
"mission_gemini_auth" => question::handle(state, name, args).await
n if n.starts_with("mission_question_") => question::handle(state, n, args).await
n if n.starts_with("mission_incident_") => question::handle(state, n, args).await
n if n == "mission_health"
`);

  writeFixture(root, DEFAULT_FILES.facade, `
mod auth;
mod decision;
mod incident;
mod llm_trace;
mod question_flow;
"mission_question" => question_flow::handle_consolidated(state, args).await
"mission_decision_stats" => decision::handle_stats(state, args).await
"mission_llm_trace" => llm_trace::handle(state, args).await
"mission_gemini_auth" => auth::handle(state, args).await
"mission_incident" => incident::handle_consolidated(state, args).await
n if n.starts_with("mission_incident_") => incident::handle_legacy(state, n, args).await
Unknown question tool
`);

  writeFixture(root, DEFAULT_FILES.questionFlow, `
QuestionCreateArgs QuestionListArgs QuestionAnswerArgs QuestionIdArgs
handle_consolidated handle_legacy handle_create handle_list handle_get handle_answer handle_dismiss
list_running_autopilot_tasks CreateAgentQuestionInput create_agent_question list_agent_questions
get_agent_question answer_agent_question dismiss_agent_question
QuestionEvent::Created QuestionEvent::Resolved TaskEvent::Completed
`);

  writeFixture(root, DEFAULT_FILES.decision, 'handle_stats decision_stats hours');

  writeFixture(root, DEFAULT_FILES.llmTrace, `
gemini_trace mission_gemini_trace gemini_stats mission_gemini_stats gemini_watch watch_action
mission_gemini_watch gemini_auth mission_gemini_auth jarvis_logs mission_jarvis_logs
jarvis_trace mission_jarvis_trace
`);

  writeFixture(root, DEFAULT_FILES.auth, 'mission_gemini_auth crate::handlers::misc::handle');

  writeFixture(root, DEFAULT_FILES.incident, `
handle_consolidated handle_legacy mission_incident_test mission_incident_list mission_incident_get
mission_incident_remediate mission_incident_status mission_incident_close Unknown incident tool
`);

  writeFixture(root, DEFAULT_FILES.sysinfraMisc, `
mission_incident_test mission_incident_list incident_get incident_remediate incident_status
incident_close mission_gemini_auth publish_incident triage_incident is_safe_to_close_task
`);

  writeFixture(root, DEFAULT_FILES.mcp, `
ToolDefinition::new
"mission_question" "mission_llm_trace" "mission_decision_stats" "mission_gemini_auth" "mission_incident"
"create" "answer" "dismiss" "remediate" "close"
`);

  return root;
}

function writeFixture(root, rel, source) {
  const abs = path.join(root, rel);
  fs.mkdirSync(path.dirname(abs), { recursive: true });
  fs.writeFileSync(abs, source);
}

if (import.meta.url === `file://${process.argv[1]}`) {
  main();
}
