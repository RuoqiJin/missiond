#!/usr/bin/env node

import fs from 'node:fs';
import path from 'node:path';
import { spawnSync } from 'node:child_process';

const args = parseArgs(process.argv.slice(2));
const cwd = path.resolve(args.cwd ?? process.cwd());
const message = String(args.message ?? process.env.CODEX_USER_REQUEST ?? '').trim();
const repoRoot = findRepoRoot(cwd);
const jsonOutput = Boolean(args.json);
const liveRequested = Boolean(args.live) || process.env.MISSION_CONTEXT_PACK_LIVE === '1';

if (!repoRoot) {
  emit({
    schema: 'missiond.codex-app-context-pack.v1',
    generated_at: new Date().toISOString(),
    cwd,
    message,
    diagnostics: [{ level: 'error', message: 'Not inside a MissionD repository checkout.' }],
  });
  process.exitCode = 2;
} else {
  emit(await buildContextPack({ repoRoot, cwd, message, liveRequested }));
}

async function buildContextPack({ repoRoot, cwd, message, liveRequested }) {
  const text = normalize(message);
  const project = inferProject({ repoRoot, cwd, text });
  const intent = inferIntent(text);
  const authority = buildAuthority(intent, repoRoot);
  const suggestedFirstReads = unique([
    ...authority.read_order,
    ...intent.flatMap((candidate) => candidate.first_reads ?? []),
  ]).filter((rel) => exists(repoRoot, rel));
  const evidenceLanes = buildEvidenceLanes(intent);
  const knownFacts = buildKnownFacts(intent);
  const nextActions = buildNextActions(intent, suggestedFirstReads);

  const pack = {
    schema: 'missiond.codex-app-context-pack.v1',
    context_authority: 'fallback-hints-only',
    generated_at: new Date().toISOString(),
    cwd,
    repo_root: repoRoot,
    message,
    project,
    intent_candidates: intent.map(({ first_reads, ...candidate }) => candidate),
    authority,
    suggested_first_reads: suggestedFirstReads,
    evidence_lanes: evidenceLanes,
    known_facts: knownFacts,
    next_actions: nextActions,
    avoid_first_reads: [
      'Do not start with a broad repo-root rg when V3 SSOT or a scoped lane exists.',
      'Do not search .next, node_modules, target, or cold runtime archives unless the task explicitly asks for build/runtime forensics.',
      'Do not treat raw conversation/provider logs as startup context; use conversation_audit or explicit raw opt-in.',
      'Do not infer MissionD design authority from Rust/TypeScript before checking the V3 contract.',
    ],
    missiond_surfaces: {
      boot: {
        tool: 'mission_context_boot',
        local_call: 'node scripts/mission-mcp-call.mjs mission_context_boot \'{"project_id":"missiond"}\'',
      },
      gather: {
        tool: 'mission_context_gather',
        local_call: `node scripts/mission-mcp-call.mjs mission_context_gather '${JSON.stringify({
          query: message || 'MissionD current task context',
          source_profile: pickSourceProfile(intent),
          project_id: project.id,
          limit: 8,
        }).replaceAll("'", "'\\''")}'`,
      },
      note: 'Use these MCP-backed surfaces for live/runtime evidence when available; this script is the deterministic Codex App bootstrap fallback.',
    },
    diagnostics: diagnosticsFor({ repoRoot, suggestedFirstReads }),
  };
  if (liveRequested) {
    pack.live_context = gatherLiveContext({ repoRoot, message, project, intent });
  }
  return pack;
}

function buildAuthority(intent, repoRoot) {
  const readOrder = [
    '.missiond/v3/missiond-blueprint.lisp',
    '.missiond/v3/shards/index.lisp',
  ];
  const activeAuthoring = [
    '.missiond/v3/shards',
    '.missiond/frontend',
    'crates',
    'packages',
    'scripts',
  ];
  if (hasIntent(intent, 'codex_boot_context')) {
    readOrder.push(
      '.missiond/v3/shards/workstation-runtime.lisp',
      '.missiond/v3/evidence/codex-boot-context.lisp',
      'crates/missiond-mcp/src/tools/knowledge/context_gather.rs',
      'crates/missiond-daemon/src/handlers/knowledge/context_gather.rs',
    );
  }
  if (hasIntent(intent, 'memory_context_pack') || hasIntent(intent, 'conversation_audit')) {
    readOrder.push('.missiond/v3/shards/memory-knowledge-runtime.lisp');
  }
  if (hasIntent(intent, 'xjpcode_chat_cockpit') || hasIntent(intent, 'xjpcode_worker_dispatch')) {
    readOrder.push('.missiond/v3/shards/workstation-runtime.lisp');
  }
  if (hasIntent(intent, 'board_frontend_change')) {
    readOrder.push(
      '.missiond/frontend/board-blueprint.lisp',
      'packages/board/src/App.tsx',
      'packages/board/src/components',
      'packages/board/src/app/api',
    );
  }
  if (hasIntent(intent, 'conversation_audit')) {
    readOrder.push(
      'packages/board/src/app/api/conversations/route.ts',
      'packages/board/src/components/Conversations.tsx',
      'crates/missiond-daemon/src/workers/local/codex_ingestion_worker.rs',
    );
  }
  return {
    read_order: unique(readOrder),
    active_authoring_paths: activeAuthoring.filter((rel) => exists(repoRoot, rel)),
    policy: 'V3 SSOT first, then compiled/runtime ABI, then implementation. Use runtime probes only after the contract path is known.',
  };
}

function inferIntent(text) {
  const candidates = [];
  const add = (candidate) => candidates.push(candidate);
  const mentionsMissiond = hasAny(text, ['missiond', 'missond', 'mission d', 'misson d']);
  const mentionsXjpcode = hasAny(text, ['xjpcode', 'xjp code', 'xjp-code', 'xjp_code']);
  const mentionsCodex = hasAny(text, ['codex', 'codex cli', 'codex app']);
  const mentionsContext = hasAny(text, ['context', '上下文', 'context pack', '上下文包', '注入', '提示词']);
  const mentionsFrontend = hasAny(text, ['frontend', 'front-end', '前端', '页面', 'ui', '面板', 'tab', '选项卡']);
  const mentionsChat = hasAny(text, ['chat', '聊天', '对话', 'completions', '/v1/chat/completions']);
  const mentionsWorker = hasAny(text, ['worker', 'dispatch', 'delegate', '工位', '派发', '四路', '并行', 'boardtask']);
  const mentionsConversation = hasAny(text, ['jsonl', 'conversation', '对话记录', '消息', '工具调用', '会话']);
  const wantsAnalysis = hasAny(text, ['分析', '调查', '调研', '为什么', '如何', 'review', 'audit']);

  if (mentionsContext || (mentionsCodex && mentionsMissiond)) {
    add({
      id: 'codex_boot_context',
      confidence: mentionsContext ? 0.95 : 0.75,
      why: 'The request is about improving Codex startup/context behavior inside MissionD.',
      first_reads: [
        '.missiond/v3/shards/workstation-runtime.lisp',
        '.missiond/v3/evidence/codex-boot-context.lisp',
        'crates/missiond-mcp/src/tools/knowledge/context_gather.rs',
        'crates/missiond-daemon/src/handlers/knowledge/context_gather.rs',
      ],
    });
    add({
      id: 'memory_context_pack',
      confidence: 0.85,
      why: 'Context packs are governed by evidence-lane and memory/runtime retrieval rules.',
      first_reads: [
        '.missiond/v3/shards/memory-knowledge-runtime.lisp',
        'scripts/check-v3-memory-kb-isomorphism.mjs',
      ],
    });
  }
  if (mentionsXjpcode && (mentionsChat || mentionsFrontend)) {
    add({
      id: 'xjpcode_chat_cockpit',
      confidence: 0.9,
      why: 'The request mentions using xjpcode in a UI/chat surface, not necessarily worker delegation.',
      guardrail: 'Prefer the chat/provider cockpit framing until the user asks for task dispatch, read_scope/write_scope, or BoardTask fanout.',
      first_reads: [
        '.missiond/v3/shards/workstation-runtime.lisp',
        '.missiond/frontend/board-blueprint.lisp',
        'packages/board/src/App.tsx',
      ],
    });
  }
  if (mentionsXjpcode && mentionsWorker) {
    add({
      id: 'xjpcode_worker_dispatch',
      confidence: 0.72,
      why: 'The request mentions worker/fanout/dispatch concepts; verify whether it is a controlled MissionD worker lane or just user-facing chat.',
      guardrail: 'Do not collapse chat cockpit requirements into mission_task_delegate unless the user explicitly wants MissionD to own the worker turn.',
      first_reads: [
        '.missiond/v3/shards/workstation-runtime.lisp',
        'scripts/check-v3-workstation-isomorphism.mjs',
      ],
    });
  }
  if (mentionsFrontend) {
    add({
      id: 'board_frontend_change',
      confidence: mentionsMissiond ? 0.86 : 0.68,
      why: 'The request affects the MissionD Board/frontend experience.',
      first_reads: [
        '.missiond/frontend/board-blueprint.lisp',
        'packages/board/src/App.tsx',
        'packages/board/src/components',
        'packages/board/src/app/api',
      ],
    });
  }
  if (mentionsConversation) {
    add({
      id: 'conversation_audit',
      confidence: mentionsCodex ? 0.88 : 0.7,
      why: 'The request needs Codex conversation/message/tool-call evidence.',
      first_reads: [
        '.missiond/v3/shards/memory-knowledge-runtime.lisp',
        'packages/board/src/app/api/conversations/route.ts',
        'packages/board/src/components/Conversations.tsx',
        'crates/missiond-daemon/src/workers/local/codex_ingestion_worker.rs',
      ],
    });
  }
  if (wantsAnalysis) {
    add({
      id: 'architecture_analysis',
      confidence: mentionsMissiond ? 0.76 : 0.55,
      why: 'The request asks for causal/architecture analysis, so use SSOT and typed evidence before implementation claims.',
      first_reads: [
        '.missiond/v3/missiond-blueprint.lisp',
        '.missiond/v3/shards/index.lisp',
      ],
    });
  }
  if (candidates.length === 0) {
    add({
      id: 'missiond_general',
      confidence: mentionsMissiond ? 0.65 : 0.45,
      why: 'No narrow intent matched; start from MissionD SSOT and scoped search.',
      first_reads: [
        '.missiond/v3/missiond-blueprint.lisp',
        '.missiond/v3/shards/index.lisp',
      ],
    });
  }

  return candidates.sort((a, b) => b.confidence - a.confidence);
}

function buildEvidenceLanes(intent) {
  const lanes = [
    {
      id: 'ssot_contract',
      question: 'What does V3 declare as the authority and public surface for this request?',
      first_reads: ['.missiond/v3/missiond-blueprint.lisp', '.missiond/v3/shards/index.lisp'],
      output: 'Relevant shard/function/artifact contract, with checker if present.',
    },
    {
      id: 'implementation_entrypoints',
      question: 'Which Rust/TypeScript/script paths implement the declared surface?',
      first_reads: ['crates', 'packages', 'scripts'],
      output: 'Narrow file list before patching or broad search.',
    },
  ];
  if (hasIntent(intent, 'codex_boot_context') || hasIntent(intent, 'memory_context_pack')) {
    lanes.push({
      id: 'context_bootstrap',
      question: 'Which compact context can be loaded without raw logs or prompt bloat?',
      first_reads: [
        '.missiond/v3/evidence/codex-boot-context.lisp',
        '.missiond/v3/shards/memory-knowledge-runtime.lisp',
        'crates/missiond-mcp/src/tools/knowledge/context_gather.rs',
      ],
      output: 'Boot capsule, evidence-lane profile, and whether live mission_context_gather is needed.',
    });
  }
  if (hasIntent(intent, 'xjpcode_chat_cockpit') || hasIntent(intent, 'xjpcode_worker_dispatch')) {
    lanes.push({
      id: 'xjpcode_shape',
      question: 'Is the request asking for user-facing xjpcode chat, MissionD worker dispatch, or both?',
      first_reads: ['.missiond/v3/shards/workstation-runtime.lisp', '.missiond/frontend/board-blueprint.lisp'],
      output: 'Chosen product shape and rejected interpretation.',
    });
  }
  if (hasIntent(intent, 'conversation_audit')) {
    lanes.push({
      id: 'conversation_evidence',
      question: 'Which conversation/session id and raw JSONL source are authoritative?',
      first_reads: [
        'packages/board/src/app/api/conversations/route.ts',
        'crates/missiond-daemon/src/workers/local/codex_ingestion_worker.rs',
      ],
      output: 'Conversation id, message count, tool-call grouping, and raw-source fallback.',
    });
  }
  if (hasIntent(intent, 'board_frontend_change')) {
    lanes.push({
      id: 'frontend_runtime',
      question: 'Which Board route/component/API changes are needed, and how will the page be verified?',
      first_reads: ['.missiond/frontend/board-blueprint.lisp', 'packages/board/src/App.tsx', 'packages/board/src/components'],
      output: 'Patch surface plus typecheck/browser verification.',
    });
  }
  return lanes;
}

function buildKnownFacts(intent) {
  const facts = [
    {
      id: 'context_pack_is_code_assembled',
      text: 'The seed context pack is deterministic retrieval/classification from V3 SSOT, project registry, active evidence lanes, and file/runtime probes; an LLM is optional for summarization, not required for assembling the pack.',
    },
    {
      id: 'raw_history_not_startup_context',
      text: 'MissionD policy excludes raw conversations/provider logs from default startup context; use conversation_audit/full_debug only when the user explicitly needs those raw records.',
    },
  ];
  if (hasIntent(intent, 'xjpcode_chat_cockpit')) {
    facts.push({
      id: 'xjpcode_chat_vs_worker',
      text: 'A UI where the user talks to xjpcode should be treated as a chat/provider cockpit unless the user asks MissionD to own a worker turn with scoped task contracts.',
    });
  }
  if (hasIntent(intent, 'codex_boot_context')) {
    facts.push({
      id: 'existing_boot_surfaces',
      text: 'V3 already declares mission_context_boot and mission_context_gather; this script provides a Codex App pull-based bootstrap when prompt/tool injection is not yet available.',
    });
  }
  return facts;
}

function buildNextActions(intent, suggestedFirstReads) {
  const actions = [
    'Read the suggested_first_reads before broad repository search.',
    'State the selected intent framing and any rejected framing before implementing.',
  ];
  if (hasIntent(intent, 'codex_boot_context') || hasIntent(intent, 'memory_context_pack')) {
    actions.push('If the MissionD MCP daemon is available, call mission_context_gather for live evidence after reading this pack.');
  }
  if (hasIntent(intent, 'board_frontend_change')) {
    actions.push('After frontend edits, run the relevant typecheck/build and verify the Board page in the browser.');
  }
  if (suggestedFirstReads.length === 0) {
    actions.push('No suggested files resolved; fall back to AGENTS.md MissionD architecture read order.');
  }
  return actions;
}

function diagnosticsFor({ repoRoot, suggestedFirstReads }) {
  const diagnostics = [];
  for (const rel of [
    '.missiond/v3/missiond-blueprint.lisp',
    '.missiond/v3/shards/index.lisp',
    '.missiond/v3/shards/workstation-runtime.lisp',
    '.missiond/v3/shards/memory-knowledge-runtime.lisp',
  ]) {
    if (!exists(repoRoot, rel)) diagnostics.push({ level: 'warn', message: `Missing expected MissionD authority file: ${rel}` });
  }
  if (suggestedFirstReads.length === 0) diagnostics.push({ level: 'warn', message: 'No suggested first reads resolved on disk.' });
  return diagnostics;
}

function inferProject({ repoRoot, cwd, text }) {
  const relCwd = path.relative(repoRoot, cwd);
  if (!relCwd.startsWith('..')) {
    return { id: 'missiond', root: repoRoot, confidence: 1, source: 'cwd' };
  }
  if (hasAny(text, ['missiond', 'missond', 'misson d', 'mission d'])) {
    return { id: 'missiond', root: repoRoot, confidence: 0.85, source: 'message' };
  }
  return { id: 'unknown', root: cwd, confidence: 0.2, source: 'cwd' };
}

function pickSourceProfile(intent) {
  if (hasIntent(intent, 'conversation_audit')) return 'conversation_audit';
  return 'intent_default';
}

function emit(pack) {
  if (jsonOutput) {
    console.log(JSON.stringify(pack, null, 2));
    return;
  }
  console.log(renderMarkdown(pack));
}

function renderMarkdown(pack) {
  const lines = [
    '# MissionD Codex App Context Pack',
    '',
    `schema: ${pack.schema}`,
    `generated_at: ${pack.generated_at}`,
    `repo_root: ${pack.repo_root ?? '(unknown)'}`,
    '',
    '## Intent Candidates',
    ...renderList(pack.intent_candidates ?? [], (candidate) => {
      const guardrail = candidate.guardrail ? `; guardrail: ${candidate.guardrail}` : '';
      return `${candidate.id} (${candidate.confidence}): ${candidate.why}${guardrail}`;
    }),
    '',
    '## Suggested First Reads',
    ...renderList(pack.suggested_first_reads ?? [], (item) => item),
    '',
    '## Evidence Lanes',
    ...renderList(pack.evidence_lanes ?? [], (lane) => `${lane.id}: ${lane.question} -> ${lane.output}`),
    '',
    '## Known Facts',
    ...renderList(pack.known_facts ?? [], (fact) => `${fact.id}: ${fact.text}`),
    '',
    '## Avoid First Reads',
    ...renderList(pack.avoid_first_reads ?? [], (item) => item),
    '',
    '## Next Actions',
    ...renderList(pack.next_actions ?? [], (item) => item),
  ];
  if (pack.missiond_surfaces) {
    lines.push(
      '',
      '## Optional Live Surfaces',
      `- ${pack.missiond_surfaces.boot.local_call}`,
      `- ${pack.missiond_surfaces.gather.local_call}`,
    );
  }
  if (pack.diagnostics?.length) {
    lines.push('', '## Diagnostics', ...renderList(pack.diagnostics, (item) => `${item.level}: ${item.message}`));
  }
  return lines.join('\n');
}

function renderList(items, render) {
  if (!items.length) return ['- (none)'];
  return items.map((item) => `- ${render(item)}`);
}

function parseArgs(argv) {
  const out = {};
  for (let i = 0; i < argv.length; i += 1) {
    const arg = argv[i];
    if (arg === '--json') out.json = true;
    else if (arg === '--live') out.live = true;
    else if (arg === '--message') out.message = argv[++i] ?? '';
    else if (arg.startsWith('--message=')) out.message = arg.slice('--message='.length);
    else if (arg === '--cwd') out.cwd = argv[++i] ?? '';
    else if (arg.startsWith('--cwd=')) out.cwd = arg.slice('--cwd='.length);
    else if (arg === '--help' || arg === '-h') {
      printUsage();
      process.exit(0);
    }
  }
  return out;
}

function printUsage() {
  console.log(`Usage: node scripts/mission-context-pack.mjs [--json] [--live] [--cwd <path>] [--message <latest user request>]

Builds a compact deterministic MissionD context pack for Codex App sessions.
When --message is omitted, CODEX_USER_REQUEST is used if present.
Use --live to try mission_context_boot / mission_context_gather first and keep
the deterministic pack as fallback hints when the MissionD runtime is absent.`);
}

function gatherLiveContext({ repoRoot, message, project, intent }) {
  return {
    schema: 'missiond.codex-app-live-context.v1',
    boot: callMissionMcp(repoRoot, 'mission_context_boot', {
      project_id: project.id,
      include_capsule: false,
    }),
    gather: callMissionMcp(repoRoot, 'mission_context_gather', {
      query: message || 'MissionD current task context',
      source_profile: pickSourceProfile(intent),
      project_id: project.id,
      limit: 8,
    }),
  };
}

function callMissionMcp(repoRoot, toolName, toolArgs) {
  const proc = spawnSync(
    process.execPath,
    ['scripts/mission-mcp-call.mjs', toolName, JSON.stringify(toolArgs)],
    {
      cwd: repoRoot,
      encoding: 'utf8',
      timeout: Number(process.env.MISSION_CONTEXT_PACK_LIVE_TIMEOUT_MS ?? 6000),
      env: {
        ...process.env,
        MISSION_MCP_CALL_TIMEOUT_MS: process.env.MISSION_MCP_CALL_TIMEOUT_MS ?? '5000',
      },
    },
  );
  if (proc.error) {
    return { ok: false, error: proc.error.message };
  }
  if (proc.status !== 0) {
    return {
      ok: false,
      exit_code: proc.status,
      stderr: trimText(proc.stderr),
      stdout: trimText(proc.stdout),
    };
  }
  try {
    return { ok: true, response: JSON.parse(proc.stdout) };
  } catch {
    return { ok: true, stdout: trimText(proc.stdout) };
  }
}

function trimText(text, maxChars = 4000) {
  const value = String(text ?? '').trim();
  if (value.length <= maxChars) return value;
  return `${value.slice(0, maxChars - 3)}...`;
}

function findRepoRoot(start) {
  let dir = start;
  while (dir && dir !== path.dirname(dir)) {
    if (exists(dir, '.missiond/v3/missiond-blueprint.lisp') && exists(dir, 'AGENTS.md')) return dir;
    dir = path.dirname(dir);
  }
  return null;
}

function normalize(value) {
  return String(value ?? '').toLowerCase();
}

function hasAny(text, needles) {
  return needles.some((needle) => text.includes(needle));
}

function hasIntent(intent, id) {
  return intent.some((candidate) => candidate.id === id);
}

function unique(values) {
  return [...new Set(values.filter(Boolean))];
}

function exists(root, rel) {
  return fs.existsSync(path.join(root, rel));
}
