#!/usr/bin/env node

import fs from 'node:fs';
import path from 'node:path';
import { spawnSync } from 'node:child_process';

const args = parseArgs(process.argv.slice(2));
const cwd = path.resolve(args.cwd ?? process.cwd());
const message = String(args.message ?? process.env.CODEX_USER_REQUEST ?? '').trim();
const repoRoot = findRepoRoot(cwd);
const jsonOutput = Boolean(args.json);
const offlineRequested = Boolean(args.offline)
  || Boolean(args.noLive)
  || process.env.MISSION_CONTEXT_PACK_OFFLINE === '1'
  || process.env.MISSION_CONTEXT_PACK_LIVE === '0';
const liveRequested = !offlineRequested;

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
  const gatherProjectId = pickGatherProjectId(project, intent);
  const authority = buildAuthority(intent, repoRoot);
  const suggestedFirstReads = unique([
    ...authority.read_order,
    ...intent.flatMap((candidate) => candidate.first_reads ?? []),
  ]).filter((rel) => exists(repoRoot, rel));
  const evidenceLanes = buildEvidenceLanes(intent);
  const knownFacts = buildKnownFacts(intent);
  const nextActions = buildNextActions(intent, suggestedFirstReads);
  const sourceProfile = pickSourceProfile(intent);
  const requiredToolSequence = buildRequiredToolSequence({
    message,
    intent,
    project,
    gatherProjectId,
    sourceProfile,
  });
  const navigationProfile = buildNavigationProfile({
    text,
    intent,
    project,
    gatherProjectId,
    suggestedFirstReads,
    requiredToolSequence,
  });
  const missiondSurfaces = buildMissiondSurfaces({
    project,
    requiredToolSequence,
  });

  const pack = {
    schema: 'missiond.codex-app-context-pack.v1',
    context_authority: liveRequested ? 'live-delegated-with-fallback-hints' : 'fallback-hints-only',
    generated_at: new Date().toISOString(),
    cwd,
    repo_root: repoRoot,
    message,
    project,
    intent_candidates: intent.map(({ first_reads, ...candidate }) => candidate),
    authority,
    suggested_first_reads: suggestedFirstReads,
    evidence_lanes: evidenceLanes,
    required_tool_sequence: requiredToolSequence,
    navigation_profile: navigationProfile,
    known_facts: knownFacts,
    next_actions: nextActions,
    avoid_first_reads: [
      'Do not start with a broad repo-root rg when V3 SSOT or a scoped lane exists.',
      'Do not search .next, node_modules, target, or cold runtime archives unless the task explicitly asks for build/runtime forensics.',
      'Do not treat raw conversation/provider logs as startup context; use conversation_audit or explicit raw opt-in.',
      'Do not infer MissionD design authority from Rust/TypeScript before checking the V3 contract.',
    ],
    missiond_surfaces: missiondSurfaces,
    diagnostics: diagnosticsFor({ repoRoot, suggestedFirstReads }),
  };
  if (liveRequested) {
    pack.live_context = gatherLiveContext({ repoRoot, message, project, intent, gatherProjectId });
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
  if (hasIntent(intent, 'xjp_router_provider_workflow') || hasIntent(intent, 'xjp_router_runtime_probe')) {
    readOrder.push(
      '.missiond/v3/shards/universe/project-registry.lisp',
      '.missiond/v3/shards/deployment-closure-plane.lisp',
      '.missiond/v3/shards/universe/service-runtime.lisp',
      '.missiond/v3/shards/control-plane-runtime.lisp',
    );
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
  const wantsAnalysis = hasAny(text, ['分析', '调查', '调研', '检查', '确认', '查一下', '看看', '为什么', '如何', 'review', 'audit']);
  const mentionsRouter = hasAny(text, ['router', 'xjp-router', 'xjp router', '路由']);
  const mentionsGemini = hasAny(text, ['gemini', 'gemini 3.1', 'gemini-3.1', 'gemini 3.1 pro', 'gemini-3.1-pro']);
  const mentionsTranslation = hasAny(text, ['translate', 'translation', 'translator', '翻译']);
  const mentionsWorkflow = hasAny(text, ['workflow', 'workflows', 'multi-round', 'multi round', 'multi_turn', '多轮', '渠道', '生成结果']);
  const asksRuntimeProbe = hasAny(text, ['能不能', '能否', '调用', '生成结果', 'smoke', 'health', '/v1/models', '线上', '生产', 'runtime', '运行时']);

  if (mentionsRouter && (mentionsGemini || mentionsTranslation || mentionsWorkflow || mentionsProviderRuntime(text))) {
    add({
      id: 'xjp_router_provider_workflow',
      confidence: mentionsGemini || mentionsTranslation ? 0.93 : 0.84,
      why: 'The request asks about xjp-router provider/model/workflow behavior, so first resolve the router project and workflow surface.',
      guardrail: 'Do not stop at a MissionD repo-local router-policy hit or model-name memory; verify project root, workflow source, model mapping, and runtime evidence.',
      first_reads: [
        '.missiond/v3/shards/universe/project-registry.lisp',
        '.missiond/v3/shards/deployment-closure-plane.lisp',
        '.missiond/v3/shards/universe/service-runtime.lisp',
        '.missiond/v3/shards/control-plane-runtime.lisp',
      ],
    });
  }
  if (mentionsRouter && asksRuntimeProbe) {
    add({
      id: 'xjp_router_runtime_probe',
      confidence: 0.88,
      why: 'The request asks whether the router behavior can be invoked now, which needs deploy/runtime evidence after source verification.',
      guardrail: 'Resolve xjp-router as the target project/slug before health, models, or workflow smoke probes.',
      first_reads: [
        '.missiond/v3/shards/universe/project-registry.lisp',
        '.missiond/v3/shards/deployment-closure-plane.lisp',
        '.missiond/v3/shards/universe/service-runtime.lisp',
      ],
    });
  }

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

function buildNavigationProfile({
  text,
  intent,
  project,
  gatherProjectId,
  suggestedFirstReads,
  requiredToolSequence,
}) {
  const selectedProfiles = unique(
    intent.flatMap((candidate) => profileIdsForIntent(candidate.id)),
  );
  if (selectedProfiles.length === 0) selectedProfiles.push('general_grounded_intent');

  const rejectedProfiles = buildRejectedProfiles(intent);
  const knownSurfaces = buildKnownSurfaces({ text, intent });
  const requiredQuestions = buildRequiredQuestions({ intent, knownSurfaces });
  const verificationPlan = buildVerificationPlan(intent);
  const riskFlags = buildRiskFlags({ intent, knownSurfaces, suggestedFirstReads, project, gatherProjectId });

  return {
    schema: 'missiond.codex-app-navigation-profile.v1',
    source: 'mission-context-pack',
    source_profile: pickSourceProfile(intent),
    project_id: project.id,
    gather_project_id: gatherProjectId,
    project_resolution: {
      cwd_project_id: project.id,
      gather_project_id: gatherProjectId,
      rule: project.id === gatherProjectId
        ? 'Use the cwd project for live MissionD context gather.'
        : 'The request names an external service; resolve and gather against that project before broad repo search.',
    },
    selected_profiles: selectedProfiles.map((id) => ({
      id,
      reason: navigationProfileReason(id),
    })),
    rejected_profiles: rejectedProfiles,
    known_surfaces: knownSurfaces,
    recommended_tool_sequence: requiredToolSequence,
    required_questions: requiredQuestions,
    next_reads: suggestedFirstReads.slice(0, 12).map((rel, index) => ({
      order: index + 1,
      path: rel,
      reason: nextReadReason(rel),
    })),
    verification_plan: verificationPlan,
    evidence_status: {
      deterministic_bootstrap: true,
      live_mcp_followup_recommended: true,
      raw_sources_included: false,
      note: 'This profile routes the first investigation pass; live truth still comes from mission_context_gather, runtime probes, and file reads.',
    },
    risk_flags: riskFlags,
    rule: 'Use selected_profiles to choose the first investigation lane; use rejected_profiles to avoid plausible but wrong dispatch paths.',
  };
}

function buildRequiredToolSequence({ message, intent, project, gatherProjectId, sourceProfile }) {
  const query = message || 'MissionD current task context';
  const allowedLanes = defaultAllowedLanes(sourceProfile);
  const gatherArgs = {
    query,
    source_profile: sourceProfile,
    project_id: gatherProjectId,
    limit: 8,
  };
  const memoryArgs = {
    action: 'evidence_search',
    query,
    projectId: gatherProjectId,
    lanes: allowedLanes,
    limit: 8,
  };
  const sequence = [
    {
      order: 1,
      phase: 'live_context_first',
      tool: 'mission_context_gather',
      required: true,
      purpose: 'Pull bounded live MissionD evidence lanes before manual file search.',
      args: gatherArgs,
      local_call: missionMcpLocalCall('mission_context_gather', gatherArgs),
    },
    {
      order: 2,
      phase: 'reviewed_memory_evidence',
      tool: 'mission_memory',
      action: 'evidence_search',
      required: false,
      condition: 'Use when prior MissionD decisions, reviewed memory, or compact evidence may affect the answer; do not preload raw provider logs.',
      purpose: 'Search MissionD local authority evidence lanes through the memory facade.',
      args: memoryArgs,
      local_call: missionMcpLocalCall('mission_memory', memoryArgs),
    },
  ];
  if (shouldUseRepoSearch(intent)) {
    const repoArgs = {
      query: repoSearchQueryFor({ message: query, intent }),
      source_profile: sourceProfile,
      project_id: gatherProjectId,
      limit: 12,
    };
    sequence.push({
      order: sequence.length + 1,
      phase: 'profile_aware_repo_text',
      tool: 'mission_repo_search',
      required: true,
      purpose: 'Use the MissionD profile/lane-gated repo search facade before broad shell rg.',
      args: repoArgs,
      local_call: missionMcpLocalCall('mission_repo_search', repoArgs),
    });
  }
  if (project.id !== gatherProjectId) {
    sequence.unshift({
      order: 0,
      phase: 'project_resolution',
      tool: 'mission_project',
      action: 'resolve',
      required: true,
      purpose: 'Resolve the named external project before gathering KB, runtime, Board, or repo evidence.',
      args: {
        query: gatherProjectId,
      },
      local_call: missionMcpLocalCall('mission_project', { query: gatherProjectId }),
    });
    return sequence.map((step, index) => ({ ...step, order: index + 1 }));
  }
  return sequence;
}

function buildMissiondSurfaces({ project, requiredToolSequence }) {
  const byTool = new Map();
  for (const step of requiredToolSequence) {
    const key = step.action ? `${step.tool}.${step.action}` : step.tool;
    byTool.set(key, step);
    byTool.set(step.tool, step);
  }
  const bootArgs = { project_id: project.id, include_capsule: false };
  return {
    boot: {
      tool: 'mission_context_boot',
      local_call: missionMcpLocalCall('mission_context_boot', bootArgs),
    },
    gather: surfaceFromStep(byTool.get('mission_context_gather')),
    memory_evidence_search: surfaceFromStep(byTool.get('mission_memory.evidence_search')),
    repo_search: surfaceFromStep(byTool.get('mission_repo_search')),
    note: 'Use these MCP-backed surfaces before broad shell search. This script defaults to live mission_context_boot / mission_context_gather when runtime is reachable; use --offline for deterministic fallback only.',
  };
}

function surfaceFromStep(step) {
  if (!step) return null;
  return {
    tool: step.tool,
    action: step.action,
    required: step.required,
    purpose: step.purpose,
    args: step.args,
    local_call: step.local_call,
  };
}

function shouldUseRepoSearch(intent) {
  return [
    'architecture_analysis',
    'board_frontend_change',
    'xjpcode_chat_cockpit',
    'xjpcode_worker_dispatch',
    'xjp_router_provider_workflow',
    'xjp_router_runtime_probe',
    'codex_boot_context',
    'memory_context_pack',
    'conversation_audit',
  ].some((id) => hasIntent(intent, id));
}

function repoSearchQueryFor({ message, intent }) {
  if (hasIntent(intent, 'xjp_router_provider_workflow')) return 'gemini translation workflow router provider model';
  if (hasIntent(intent, 'xjpcode_chat_cockpit')) return 'xjpcode provider chat cockpit proxy';
  if (hasIntent(intent, 'codex_boot_context') || hasIntent(intent, 'memory_context_pack')) {
    return 'mission_context_gather mission_repo_search context pack memory evidence';
  }
  if (hasIntent(intent, 'conversation_audit')) return 'conversation jsonl tool call codex ingestion';
  return trimText(message, 240);
}

function defaultAllowedLanes(sourceProfile) {
  if (sourceProfile === 'deploy_ops') {
    return ['runtime_truth', 'project_ssot', 'reviewed_kb', 'active_board', 'support_refs', 'skill_evidence'];
  }
  if (sourceProfile === 'conversation_audit') {
    return ['runtime_truth', 'project_ssot', 'reviewed_kb', 'active_board', 'support_refs', 'conversation_audit'];
  }
  return ['runtime_truth', 'project_ssot', 'reviewed_kb', 'active_board', 'support_refs'];
}

function missionMcpLocalCall(tool, args) {
  return `node scripts/mission-mcp-call.mjs ${tool} '${shellQuoteJson(args)}'`;
}

function shellQuoteJson(value) {
  return JSON.stringify(value).replaceAll("'", "'\\''");
}

function profileIdsForIntent(id) {
  switch (id) {
    case 'xjp_router_provider_workflow':
      return ['router_provider_workflow'];
    case 'xjp_router_runtime_probe':
      return ['runtime_smoke_probe'];
    case 'xjpcode_chat_cockpit':
      return ['provider_chat_cockpit'];
    case 'xjpcode_worker_dispatch':
      return ['worker_dispatch_request'];
    case 'board_frontend_change':
      return ['frontend_surface_change'];
    case 'conversation_audit':
      return ['conversation_audit_request'];
    case 'codex_boot_context':
    case 'memory_context_pack':
      return ['context_bootstrap'];
    case 'architecture_analysis':
      return ['architecture_analysis'];
    default:
      return [];
  }
}

function navigationProfileReason(id) {
  const reasons = {
    provider_chat_cockpit: 'The user is asking for an in-page provider/chat experience rather than a background task lane.',
    worker_dispatch_request: 'The request uses worker, dispatch, BoardTask, or fanout language; verify task-contract boundaries before delegating.',
    frontend_surface_change: 'The request changes the MissionD Board UI or frontend API surface.',
    conversation_audit_request: 'The request needs durable conversation/message/tool-call evidence.',
    context_bootstrap: 'The request is about Codex/MissionD context injection or compact context packaging.',
    router_provider_workflow: 'The request names router/provider/model/workflow behavior; resolve xjp-router and inspect workflow/model mapping first.',
    runtime_smoke_probe: 'The request asks whether a routed behavior can be invoked now, so runtime/deploy evidence and a scoped smoke come after source verification.',
    architecture_analysis: 'The request asks for causal or architecture analysis before implementation.',
    general_grounded_intent: 'No narrow lane matched; stay SSOT-first and use scoped repository search.',
  };
  return reasons[id] ?? 'Matched by deterministic context-pack intent classification.';
}

function buildRejectedProfiles(intent) {
  const rejected = [];
  if (hasIntent(intent, 'xjpcode_chat_cockpit') && !hasIntent(intent, 'xjpcode_worker_dispatch')) {
    rejected.push({
      id: 'worker_dispatch_request',
      reason: 'The wording points to a user-facing xjpcode cockpit; do not start by wiring mission_task_delegate or worker fanout.',
    });
  }
  if (hasIntent(intent, 'board_frontend_change') && !hasIntent(intent, 'conversation_audit')) {
    rejected.push({
      id: 'conversation_audit_request',
      reason: 'Frontend implementation does not require raw conversation history unless the user asks for message/tool-call audit evidence.',
    });
  }
  if (hasIntent(intent, 'conversation_audit') && !hasIntent(intent, 'board_frontend_change')) {
    rejected.push({
      id: 'frontend_surface_change',
      reason: 'A conversation audit can often be answered from indexed/raw conversation evidence before changing the Board UI.',
    });
  }
  if (hasIntent(intent, 'xjp_router_provider_workflow')) {
    rejected.push({
      id: 'missiond_internal_router_policy_only',
      reason: 'A router provider/workflow request must resolve the xjp-router project/runtime; a MissionD router-policy hit alone is not enough.',
    });
  }
  return rejected;
}

function buildKnownSurfaces({ text, intent }) {
  const surfaces = [];
  if (hasIntent(intent, 'xjpcode_chat_cockpit')) {
    surfaces.push({
      id: 'xjpcode-chat-cockpit',
      class: 'provider-chat-cockpit',
      matched_terms: matchedTerms(text, ['xjpcode', 'xjp code', '网页', '页面', '面板', 'chat', '聊天']),
      authority_refs: ['.missiond/frontend/board-blueprint.lisp', '.missiond/v3/shards/workstation-runtime.lisp'],
      implementation_refs: [
        'packages/board/src/components/XjpcodePanel.tsx',
        'packages/board/src/app/api/xjpcode',
        'packages/board/src/lib/xjpcodeProxy.ts',
        'packages/board/src/App.tsx',
      ],
      probes: [
        'Check whether Board already has an XJPCode tab/projection.',
        'Check provider/chat proxy route shape before adding worker dispatch.',
      ],
    });
  }
  if (hasIntent(intent, 'conversation_audit')) {
    surfaces.push({
      id: 'codex-conversation-audit',
      class: 'conversation-evidence',
      matched_terms: matchedTerms(text, ['codex', 'conversation', 'jsonl', '消息', '工具调用', '会话']),
      authority_refs: ['.missiond/v3/shards/memory-knowledge-runtime.lisp'],
      implementation_refs: [
        'packages/board/src/app/api/conversations/route.ts',
        'packages/board/src/components/Conversations.tsx',
        'crates/missiond-daemon/src/workers/local/codex_ingestion_worker.rs',
      ],
      probes: [
        'Resolve conversation id before raw JSONL reads.',
        'Group tool-call rows before expanding raw payloads.',
      ],
    });
  }
  if (hasIntent(intent, 'xjp_router_provider_workflow')) {
    surfaces.push({
      id: 'xjp-router-provider-workflow',
      class: 'external-service-provider-workflow',
      matched_terms: matchedTerms(text, ['router', 'xjp-router', 'gemini', 'translate', 'translation', '翻译', 'workflow', '多轮', '渠道']),
      authority_refs: [
        '.missiond/v3/shards/universe/project-registry.lisp',
        '.missiond/v3/shards/deployment-closure-plane.lisp',
        '.missiond/v3/shards/universe/service-runtime.lisp',
        '.missiond/v3/shards/control-plane-runtime.lisp',
      ],
      implementation_refs: [
        'mission_project resolve query=xjp-router',
        'resolved xjp-router root/.missiond/intent.lisp',
        'resolved xjp-router root/.missiond/backend/router-workflow-blueprint.lisp',
        'resolved xjp-router root/src/extra/workflows',
        'resolved xjp-router root/config/xjp.toml',
      ],
      probes: [
        'Resolve xjp-router before reading router implementation files.',
        'Verify the workflow source and provider/model mapping before runtime smoke.',
        'Treat model-name memory as a hint, not proof that the current workflow is callable.',
      ],
    });
  }
  if (hasIntent(intent, 'xjp_router_runtime_probe')) {
    surfaces.push({
      id: 'xjp-router-runtime',
      class: 'deploy-runtime-probe',
      matched_terms: matchedTerms(text, ['router', 'xjp-router', '调用', '生成结果', 'smoke', 'health', '/v1/models', '运行时']),
      authority_refs: [
        '.missiond/v3/shards/universe/project-registry.lisp',
        '.missiond/v3/shards/deployment-closure-plane.lisp',
      ],
      implementation_refs: [
        'mission_context_gather source_profile=deploy_ops project_id=xjp-router',
        'Deploy Center service slug xjp-router',
      ],
      probes: [
        'Resolve xjp-router deployment identity before health/model probes.',
        'Check deploy evidence and runtime health before a workflow call.',
        'Run a scoped smoke without printing credentials or prompt secrets.',
      ],
    });
  }
  if (hasIntent(intent, 'codex_boot_context') || hasIntent(intent, 'memory_context_pack')) {
    surfaces.push({
      id: 'codex-app-bootstrap-context',
      class: 'context-bootstrap',
      matched_terms: matchedTerms(text, ['codex', 'context', '上下文', '上下文包', '注入']),
      authority_refs: [
        '.missiond/v3/evidence/codex-boot-context.lisp',
        '.missiond/v3/shards/memory-knowledge-runtime.lisp',
      ],
      implementation_refs: [
        'scripts/mission-context-pack.mjs',
        'crates/missiond-daemon/src/handlers/knowledge/context_gather.rs',
        'crates/missiond-mcp/src/tools/knowledge/context_gather.rs',
      ],
      probes: [
        'Use deterministic bootstrap first; call live mission_context_gather only for runtime evidence.',
      ],
    });
  }
  return surfaces;
}

function buildRequiredQuestions({ intent, knownSurfaces }) {
  const questions = [
    'Which selected profile is load-bearing for the first turn?',
    'Which SSOT shard or frontend blueprint declares the intended surface?',
  ];
  if (hasIntent(intent, 'xjpcode_chat_cockpit')) {
    questions.push('Is this a user-facing xjpcode chat/provider cockpit, or a MissionD-owned worker dispatch lane?');
  }
  if (hasIntent(intent, 'conversation_audit')) {
    questions.push('Which conversation id or JSONL thread is authoritative for the requested evidence?');
  }
  if (hasIntent(intent, 'xjp_router_provider_workflow')) {
    questions.push('Which resolved xjp-router project root owns the router workflow source?');
    questions.push('Which workflow file maps the translation/provider behavior to Gemini or another model?');
  }
  if (hasIntent(intent, 'xjp_router_runtime_probe')) {
    questions.push('Which runtime/deployment endpoint proves xjp-router can be called now?');
    questions.push('What scoped credential or local proxy can smoke the workflow without exposing secrets?');
  }
  if (knownSurfaces.length === 0) {
    questions.push('What project/surface should be resolved before repo search?');
  }
  return unique(questions);
}

function buildVerificationPlan(intent) {
  const plan = ['Confirm selected profile against first file reads before broad search.'];
  if (hasIntent(intent, 'board_frontend_change')) {
    plan.push('After edits, run Board typecheck/build and browser verification for the changed route.');
  }
  if (hasIntent(intent, 'xjpcode_chat_cockpit')) {
    plan.push('Verify provider/chat route behavior separately from worker dispatch behavior.');
  }
  if (hasIntent(intent, 'conversation_audit')) {
    plan.push('Verify message count, chronological order, and tool-call grouping against the durable conversation source.');
  }
  if (hasIntent(intent, 'xjp_router_provider_workflow')) {
    plan.push('Resolve xjp-router through the project registry before opening router implementation files.');
    plan.push('Verify workflow source and model/provider mapping before judging current availability.');
  }
  if (hasIntent(intent, 'xjp_router_runtime_probe')) {
    plan.push('Use deploy_ops context for xjp-router, then probe health/models/workflow with a secret-safe smoke.');
  }
  if (hasIntent(intent, 'codex_boot_context') || hasIntent(intent, 'memory_context_pack')) {
    plan.push('Compare deterministic bootstrap output with live mission_context_gather when the daemon is available.');
  }
  return unique(plan);
}

function buildRiskFlags({ intent, knownSurfaces, suggestedFirstReads, project, gatherProjectId }) {
  const flags = [];
  if (hasIntent(intent, 'xjpcode_chat_cockpit') && hasIntent(intent, 'xjpcode_worker_dispatch')) {
    flags.push({
      id: 'ambiguous_xjpcode_shape',
      severity: 'medium',
      reason: 'Both cockpit and worker-dispatch language matched; ask/verify shape before implementation.',
    });
  }
  if (knownSurfaces.length === 0) {
    flags.push({
      id: 'no_known_surface_match',
      severity: 'medium',
      reason: 'No specific MissionD surface matched; start with project resolution and V3 read order.',
    });
  }
  if (suggestedFirstReads.length === 0) {
    flags.push({
      id: 'missing_first_reads',
      severity: 'high',
      reason: 'The deterministic first-read list resolved to zero files on disk.',
    });
  }
  if (
    (hasIntent(intent, 'xjp_router_provider_workflow') || hasIntent(intent, 'xjp_router_runtime_probe'))
    && project.id !== gatherProjectId
  ) {
    flags.push({
      id: 'external_project_resolution_required',
      severity: 'medium',
      reason: `The cwd project is ${project.id}, but this request should gather live context for ${gatherProjectId}.`,
    });
  }
  return flags;
}

function matchedTerms(text, needles) {
  return needles.filter((needle) => text.includes(needle));
}

function nextReadReason(rel) {
  if (rel.includes('.missiond/v3')) return 'V3 SSOT/contract authority.';
  if (rel.includes('.missiond/frontend')) return 'Board frontend SSOT/projection authority.';
  if (rel.includes('packages/board')) return 'Board frontend implementation surface.';
  if (rel.includes('crates/missiond-daemon')) return 'Daemon runtime/tool handler implementation.';
  if (rel.includes('crates/missiond-mcp')) return 'MCP tool definition surface.';
  if (rel.includes('scripts/')) return 'Checker or deterministic bootstrap implementation.';
  return 'Scoped implementation or support file selected by intent classification.';
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
  if (hasIntent(intent, 'xjp_router_provider_workflow')) {
    lanes.push({
      id: 'router_provider_workflow',
      question: 'Which xjp-router project root, workflow source, and provider/model mapping own the requested route?',
      first_reads: [
        '.missiond/v3/shards/universe/project-registry.lisp',
        '.missiond/v3/shards/deployment-closure-plane.lisp',
        '.missiond/v3/shards/universe/service-runtime.lisp',
      ],
      output: 'xjp-router project identity, workflow implementation path, model/provider mapping, and rejected MissionD-internal-only interpretation.',
    });
  }
  if (hasIntent(intent, 'xjp_router_runtime_probe')) {
    lanes.push({
      id: 'router_runtime_smoke',
      question: 'Can the resolved xjp-router runtime invoke the requested workflow now?',
      first_reads: [
        '.missiond/v3/shards/deployment-closure-plane.lisp',
        '.missiond/v3/shards/universe/service-runtime.lisp',
      ],
      output: 'Deployment identity, health/model probe result, and secret-safe smoke result or blocker.',
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
  if (hasIntent(intent, 'xjp_router_provider_workflow')) {
    facts.push({
      id: 'xjp_router_project_first',
      text: 'Router provider/workflow requests should first resolve xjp-router as the target project/service; the MissionD cwd is the control plane, not necessarily the router implementation root.',
    });
    facts.push({
      id: 'workflow_availability_requires_runtime_proof',
      text: 'A model-name or memory hit is only a hint; current availability requires workflow source, model/provider mapping, and runtime smoke evidence.',
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
  if (hasIntent(intent, 'xjp_router_provider_workflow') || hasIntent(intent, 'xjp_router_runtime_probe')) {
    actions.push('Resolve xjp-router with mission_project before reading external router source files.');
    actions.push('Use mission_context_gather source_profile=deploy_ops project_id=xjp-router before runtime smoke.');
    actions.push('For workflow availability, verify source mapping and then smoke with a scoped, secret-safe call.');
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
  if (hasIntent(intent, 'xjp_router_provider_workflow') || hasIntent(intent, 'xjp_router_runtime_probe')) return 'deploy_ops';
  return 'intent_default';
}

function pickGatherProjectId(project, intent) {
  if (hasIntent(intent, 'xjp_router_provider_workflow') || hasIntent(intent, 'xjp_router_runtime_probe')) return 'xjp-router';
  return project.id;
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
    '## Required Tool Sequence',
    ...renderList(pack.required_tool_sequence ?? [], (step) => {
      const action = step.action ? ` ${step.action}` : '';
      const required = step.required ? 'required' : 'conditional';
      return `${step.order}. ${step.tool}${action} (${required}): ${step.purpose}`;
    }),
    '',
    '## Navigation Profile',
    ...renderNavigationProfile(pack.navigation_profile),
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
      `- ${pack.missiond_surfaces.gather?.local_call ?? '(mission_context_gather unavailable)'}`,
      `- ${pack.missiond_surfaces.memory_evidence_search?.local_call ?? '(mission_memory evidence_search unavailable)'}`,
      `- ${pack.missiond_surfaces.repo_search?.local_call ?? '(mission_repo_search not needed for this intent)'}`,
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

function renderNavigationProfile(profile) {
  if (!profile) return ['- (none)'];
  const lines = [];
  lines.push(`- selected_profiles: ${(profile.selected_profiles ?? []).map((item) => item.id).join(', ') || '(none)'}`);
  lines.push(`- rejected_profiles: ${(profile.rejected_profiles ?? []).map((item) => item.id).join(', ') || '(none)'}`);
  lines.push(`- known_surfaces: ${(profile.known_surfaces ?? []).map((item) => item.id).join(', ') || '(none)'}`);
  for (const question of profile.required_questions ?? []) {
    lines.push(`- question: ${question}`);
  }
  for (const step of profile.verification_plan ?? []) {
    lines.push(`- verify: ${step}`);
  }
  for (const step of profile.recommended_tool_sequence ?? []) {
    const action = step.action ? ` ${step.action}` : '';
    lines.push(`- tool: ${step.order}. ${step.tool}${action} -> ${step.purpose}`);
  }
  return lines;
}

function parseArgs(argv) {
  const out = {};
  for (let i = 0; i < argv.length; i += 1) {
    const arg = argv[i];
    if (arg === '--json') out.json = true;
    else if (arg === '--live') out.live = true;
    else if (arg === '--offline' || arg === '--no-live') out.offline = true;
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
  console.log(`Usage: node scripts/mission-context-pack.mjs [--json] [--offline] [--cwd <path>] [--message <latest user request>]

Builds a compact deterministic MissionD context pack for Codex App sessions.
When --message is omitted, CODEX_USER_REQUEST is used if present.
By default it tries mission_context_boot / mission_context_gather first and keeps
the deterministic pack as fallback hints when the MissionD runtime is absent.
Use --offline or MISSION_CONTEXT_PACK_OFFLINE=1 for deterministic fallback only.`);
}

function gatherLiveContext({ repoRoot, message, project, intent, gatherProjectId }) {
  return {
    schema: 'missiond.codex-app-live-context.v1',
    boot: callMissionMcp(repoRoot, 'mission_context_boot', {
      project_id: project.id,
      include_capsule: false,
    }),
    gather: callMissionMcp(repoRoot, 'mission_context_gather', {
      query: message || 'MissionD current task context',
      source_profile: pickSourceProfile(intent),
      project_id: gatherProjectId,
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

function mentionsProviderRuntime(text) {
  return hasAny(text, ['provider', 'model', '模型', 'workflow', 'workflows', '渠道', '调用', '生成结果']);
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
