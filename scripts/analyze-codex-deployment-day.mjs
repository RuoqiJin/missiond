#!/usr/bin/env node

import { spawnSync } from 'node:child_process';
import { mkdirSync, writeFileSync } from 'node:fs';
import { dirname, resolve } from 'node:path';

function usage() {
  console.log(`Usage: node scripts/analyze-codex-deployment-day.mjs [--date YYYY-MM-DD] [--db missiond] [--gap-min 15] [--out path] [--json]
       [--strict-session SESSION_ID ...] [--expanded-session SESSION_ID ...]

Read-only Codex deployment usage analysis from MissionD Postgres tables.

Outputs:
  - all Codex message/tool-call totals for the day
  - strict deployment sessions: production/deploy-closure work
  - expanded deployment sessions: strict + new-service/privatecloud rollout work
  - raw messages for included sessions

When --strict-session or --expanded-session is provided, totals use that
reviewed session set and automatic matches are kept only as candidates.`);
}

function parseArgs(argv) {
  const opts = {
    date: new Date().toISOString().slice(0, 10),
    db: 'missiond',
    gapMin: 15,
    out: null,
    json: false,
    strictSessions: [],
    expandedSessions: [],
  };
  for (let i = 2; i < argv.length; i += 1) {
    const arg = argv[i];
    if (arg === '--help' || arg === '-h') {
      usage();
      process.exit(0);
    }
    if (arg === '--json') {
      opts.json = true;
      continue;
    }
    const next = argv[i + 1];
    if (arg === '--date' && next) {
      opts.date = next;
      i += 1;
      continue;
    }
    if (arg === '--db' && next) {
      opts.db = next;
      i += 1;
      continue;
    }
    if (arg === '--gap-min' && next) {
      opts.gapMin = Number(next);
      i += 1;
      continue;
    }
    if (arg === '--out' && next) {
      opts.out = next;
      i += 1;
      continue;
    }
    if (arg === '--strict-session' && next) {
      opts.strictSessions.push(next);
      i += 1;
      continue;
    }
    if (arg === '--expanded-session' && next) {
      opts.expandedSessions.push(next);
      i += 1;
      continue;
    }
    throw new Error(`unknown argument: ${arg}`);
  }
  if (!/^\d{4}-\d{2}-\d{2}$/.test(opts.date)) {
    throw new Error('--date must be YYYY-MM-DD');
  }
  if (!/^[A-Za-z0-9_.-]+$/.test(opts.db)) {
    throw new Error('--db contains unsupported characters');
  }
  if (!Number.isFinite(opts.gapMin) || opts.gapMin <= 0 || opts.gapMin > 240) {
    throw new Error('--gap-min must be between 1 and 240');
  }
  return opts;
}

function psqlJson(db, sql) {
  const wrapped = `SELECT COALESCE(json_agg(t), '[]'::json) FROM (${sql}) t;`;
  const result = spawnSync('psql', ['-d', db, '-v', 'ON_ERROR_STOP=1', '-At', '-c', wrapped], {
    encoding: 'utf8',
    maxBuffer: 256 * 1024 * 1024,
  });
  if (result.status !== 0) {
    throw new Error(`psql failed (${result.status})\nSTDOUT:\n${result.stdout}\nSTDERR:\n${result.stderr}`);
  }
  return JSON.parse(result.stdout.trim() || '[]');
}

function psqlScalar(db, sql) {
  const result = spawnSync('psql', ['-d', db, '-v', 'ON_ERROR_STOP=1', '-At', '-c', sql], {
    encoding: 'utf8',
    maxBuffer: 64 * 1024 * 1024,
  });
  if (result.status !== 0) {
    throw new Error(`psql failed (${result.status})\nSTDOUT:\n${result.stdout}\nSTDERR:\n${result.stderr}`);
  }
  return result.stdout.trim();
}

function sqlBounds(date) {
  return `WITH bounds AS (
    SELECT timestamptz '${date} 00:00:00+08' AS start_ts,
           timestamptz '${date} 00:00:00+08' + interval '1 day' AS end_ts
  )`;
}

function cleanForClassification(text) {
  const raw = String(text || '');
  const trimmed = raw.trimStart();
  if (
    trimmed.startsWith('<permissions instructions>') ||
    trimmed.startsWith('<environment_context>') ||
    trimmed.startsWith('# AGENTS.md instructions') ||
    trimmed.includes('# Codex desktop context') ||
    trimmed.includes('Knowledge cutoff:')
  ) {
    return '';
  }
  return raw;
}

function countRegex(text, re) {
  const matches = String(text || '').match(re);
  return matches ? matches.length : 0;
}

function classifySession(sessionId, messages) {
  const userText = messages
    .filter((m) => ['user', 'worker_user'].includes(m.role))
    .map((m) => cleanForClassification(m.content))
    .join('\n');
  const cleanText = messages.map((m) => cleanForClassification(m.content)).join('\n');
  const combined = `${userText}\n${cleanText}`;

  const strictRules = [
    {
      topic: 'ASR/SpeechScribe/payments production closure',
      re: /\bASR\b|speechscribe|payments?|job store|payment reservation|扣费|充值|激活码|生产健康|production health/i,
    },
    {
      topic: 'Search Center/router deployment validation',
      re: /Search Center|source quality gate|Deploy Center trigger|Build & Push|production strict smoke|生产 smoke|provider registry.*ROUTER_XJP_KEY/i,
    },
    {
      topic: 'MissionD/XJP deployment closure architecture',
      re: /部署闭包|Deploy Center|deploy agent|service\.manifest|canary|docker-compose|compose|image digest|xjp-payments|payments\/health|legacy schema adoption/i,
    },
  ];
  const expandedRules = [
    {
      topic: 'Domain/Mail backend rollout and privatecloud deploy-agent',
      re: /xjp-domain-service|xjp-mail-service|domains\.xiaojins|mailbox|Google Workspace|privatecloud|Secret Store|GHCR|DEPLOY_GITHUB_PAT|registry login|deploy-agent/i,
    },
    {
      topic: 'Good Things Daily new service deployment',
      re: /good-things-daily|Good Things Daily|世界好事日报|goodnews\.xiaojinpro|Vercel|push,部署到 vercel|GCP VM 有/i,
    },
  ];
  // Specific new-service rollout rules run before broad production/payment
  // rules so "this service will not use payments yet" does not classify a new
  // service launch as payments production work.
  for (const rule of expandedRules) {
    if (rule.re.test(combined)) {
      return { category: 'expanded_extra', topic: rule.topic };
    }
  }
  for (const rule of strictRules) {
    if (rule.re.test(userText) || (rule.re.test(combined) && /deploy|部署|上线|smoke|canary|Build & Push|production/i.test(combined))) {
      return { category: 'strict', topic: rule.topic };
    }
  }

  const hardKeywordHits = countRegex(
    combined,
    /deploy(?:ment|ed|ing)?|部署|上线|发布|release|smoke|rollback|回滚|privatecloud|cloud run|docker|compose|caddy|nginx|health|readiness|production|prod|staging|service\.manifest/gi,
  );
  if (hardKeywordHits >= 8) {
    return { category: 'candidate', topic: 'keyword candidate' };
  }
  return { category: 'other', topic: 'not deployment related' };
}

function bySession(rows) {
  const map = new Map();
  for (const row of rows) {
    const id = row.session_id;
    if (!map.has(id)) map.set(id, []);
    map.get(id).push(row);
  }
  return map;
}

function activeHours(events, gapMin) {
  const stamps = events
    .map((event) => Date.parse(event))
    .filter((value) => Number.isFinite(value))
    .sort((a, b) => a - b);
  if (stamps.length === 0) return { hours: 0, segments: 0, events: 0, first: null, last: null };

  const gapMs = gapMin * 60 * 1000;
  let segments = 0;
  let totalMs = 0;
  let start = stamps[0];
  let prev = stamps[0];
  for (let i = 1; i < stamps.length; i += 1) {
    const ts = stamps[i];
    if (ts - prev > gapMs) {
      segments += 1;
      totalMs += Math.max(prev - start, 60 * 1000);
      start = ts;
    }
    prev = ts;
  }
  segments += 1;
  totalMs += Math.max(prev - start, 60 * 1000);
  return {
    hours: totalMs / 3_600_000,
    segments,
    events: stamps.length,
    first: new Date(stamps[0]).toISOString(),
    last: new Date(stamps[stamps.length - 1]).toISOString(),
  };
}

function summarizeToolCalls(calls) {
  const byTool = new Map();
  let durationMs = 0;
  for (const call of calls) {
    const tool = call.tool_name || '(unknown)';
    const current = byTool.get(tool) || { toolName: tool, calls: 0, durationMs: 0 };
    const d = Number(call.duration_ms || 0);
    current.calls += 1;
    current.durationMs += d;
    durationMs += d;
    byTool.set(tool, current);
  }
  const tools = [...byTool.values()].sort((a, b) => b.calls - a.calls);
  return {
    toolCalls: calls.length,
    toolDurationMs: durationMs,
    toolDurationHours: durationMs / 3_600_000,
    tools,
  };
}

function summarizeGroup(name, sessionIds, sessionMap, toolCallsBySession, gapMin) {
  const messages = sessionIds.flatMap((id) => sessionMap.get(id)?.messages || []);
  const calls = sessionIds.flatMap((id) => toolCallsBySession.get(id) || []);
  const eventTimes = [
    ...messages.map((m) => m.timestamp),
    ...calls.map((c) => c.timestamp),
  ];
  const toolSummary = summarizeToolCalls(calls);
  return {
    name,
    sessionCount: sessionIds.length,
    messageCount: messages.length,
    userMessageCount: messages.filter((m) => ['user', 'worker_user'].includes(m.role)).length,
    ...toolSummary,
    active: activeHours(eventTimes, gapMin),
  };
}

function round(value, digits = 2) {
  return Number(value.toFixed(digits));
}

function main() {
  const opts = parseArgs(process.argv);
  const reviewedStrict = new Set(opts.strictSessions);
  const reviewedExpandedExtra = new Set(opts.expandedSessions);
  const reviewedMode = reviewedStrict.size > 0 || reviewedExpandedExtra.size > 0;
  const bounds = sqlBounds(opts.date);
  const messages = psqlJson(opts.db, `
    ${bounds}
    SELECT
      c.id AS session_id,
      c.conversation_type,
      c.status,
      c.project,
      c.model AS conversation_model,
      m.id AS message_id,
      m.role,
      m.raw_role,
      m.content,
      m.raw_content,
      m.tool_name,
      m.message_uuid,
      m.model AS message_model,
      m.timestamp
    FROM conversation_messages m
    JOIN conversations c ON c.id = m.session_id
    CROSS JOIN bounds b
    WHERE c.source = 'codex_cli'
      AND m.timestamp ~ '^20[0-9]{2}-'
      AND m.timestamp::timestamptz >= b.start_ts
      AND m.timestamp::timestamptz < b.end_ts
    ORDER BY m.timestamp::timestamptz, m.id
  `);

  const toolCalls = psqlJson(opts.db, `
    ${bounds}
    SELECT
      tc.session_id,
      tc.id AS call_id,
      tc.tool_name,
      tc.input_summary,
      tc.output_summary,
      tc.status,
      tc.duration_ms,
      tc.timestamp
    FROM conversation_tool_calls tc
    JOIN conversations c ON c.id = tc.session_id
    CROSS JOIN bounds b
    WHERE c.source = 'codex_cli'
      AND tc.timestamp::timestamptz >= b.start_ts
      AND tc.timestamp::timestamptz < b.end_ts
    ORDER BY tc.timestamp::timestamptz, tc.id
  `);

  const invalidCodexTimestampRows = Number(psqlScalar(opts.db, `
    SELECT COUNT(*)
    FROM conversation_messages m
    JOIN conversations c ON c.id = m.session_id
    WHERE c.source = 'codex_cli'
      AND NOT (m.timestamp ~ '^20[0-9]{2}-');
  `) || '0');

  const groupedMessages = bySession(messages);
  const toolCallsBySession = bySession(toolCalls);
  const sessionMap = new Map();
  for (const [sessionId, sessionMessages] of groupedMessages) {
    const classification = classifySession(sessionId, sessionMessages);
    const autoCategory = classification.category;
    const autoTopic = classification.topic;
    if (reviewedMode) {
      if (reviewedStrict.has(sessionId)) {
        classification.category = 'strict';
        classification.topic = 'reviewed strict deployment session';
      } else if (reviewedExpandedExtra.has(sessionId)) {
        classification.category = 'expanded_extra';
        classification.topic = 'reviewed expanded deployment-adjacent session';
      } else if (classification.category === 'strict' || classification.category === 'expanded_extra') {
        classification.category = 'candidate';
        classification.topic = `${autoTopic} (auto candidate, excluded from reviewed totals)`;
      }
    }
    const calls = toolCallsBySession.get(sessionId) || [];
    const toolSummary = summarizeToolCalls(calls);
    const timestamps = [
      ...sessionMessages.map((m) => m.timestamp),
      ...calls.map((c) => c.timestamp),
    ].filter(Boolean);
    const first = timestamps.length ? new Date(Math.min(...timestamps.map((t) => Date.parse(t)))).toISOString() : null;
    const last = timestamps.length ? new Date(Math.max(...timestamps.map((t) => Date.parse(t)))).toISOString() : null;
    sessionMap.set(sessionId, {
      sessionId,
      category: classification.category,
      topic: classification.topic,
      autoCategory,
      autoTopic,
      status: sessionMessages[0]?.status || null,
      project: sessionMessages[0]?.project || null,
      model: sessionMessages[0]?.conversation_model || null,
      first,
      last,
      messages: sessionMessages,
      messageCount: sessionMessages.length,
      userMessageCount: sessionMessages.filter((m) => ['user', 'worker_user'].includes(m.role)).length,
      ...toolSummary,
      rawUserPrompts: sessionMessages
        .filter((m) => ['user', 'worker_user'].includes(m.role))
        .map((m) => ({ timestamp: m.timestamp, role: m.role, content: m.content })),
    });
  }

  const strictIds = [...sessionMap.values()]
    .filter((session) => session.category === 'strict')
    .map((session) => session.sessionId);
  const expandedIds = [...sessionMap.values()]
    .filter((session) => session.category === 'strict' || session.category === 'expanded_extra')
    .map((session) => session.sessionId);

  const allToolSummary = summarizeToolCalls(toolCalls);
  const report = {
    schema: 'missiond.codex-deployment-usage-report.v1',
    generatedAt: new Date().toISOString(),
    date: opts.date,
    timezone: 'Asia/Shanghai',
    database: opts.db,
    method: {
      source: 'codex_cli',
      messageWindow: 'conversation_messages.timestamp, normal 20xx ISO timestamps only',
      toolWindow: 'conversation_tool_calls.timestamp',
      activeTime: `${opts.gapMin} minute inactivity gap over message + tool-call timestamps`,
      strictDefinition: 'production deployment, deploy closure, Deploy Center, canary/smoke, ASR/SpeechScribe/payments, Search Center deploy validation',
      expandedDefinition: 'strict plus new service rollout, domain/mail backend, privatecloud deploy-agent work',
      reviewedMode,
      reviewedStrictSessions: [...reviewedStrict],
      reviewedExpandedExtraSessions: [...reviewedExpandedExtra],
    },
    dataQuality: {
      invalidCodexTimestampRowsSkipped: invalidCodexTimestampRows,
    },
    totals: {
      codexSessions: groupedMessages.size,
      codexMessages: messages.length,
      codexUserMessages: messages.filter((m) => ['user', 'worker_user'].includes(m.role)).length,
      ...allToolSummary,
    },
    strict: summarizeGroup('strict', strictIds, sessionMap, toolCallsBySession, opts.gapMin),
    expanded: summarizeGroup('expanded', expandedIds, sessionMap, toolCallsBySession, opts.gapMin),
    sessions: [...sessionMap.values()]
      .filter((session) => ['strict', 'expanded_extra', 'candidate'].includes(session.category))
      .map((session) => ({
        sessionId: session.sessionId,
        category: session.category,
        topic: session.topic,
        autoCategory: session.autoCategory,
        autoTopic: session.autoTopic,
        status: session.status,
        project: session.project,
        model: session.model,
        first: session.first,
        last: session.last,
        messageCount: session.messageCount,
        userMessageCount: session.userMessageCount,
        toolCalls: session.toolCalls,
        toolDurationHours: round(session.toolDurationHours),
        tools: session.tools.slice(0, 20).map((tool) => ({
          toolName: tool.toolName,
          calls: tool.calls,
          durationHours: round(tool.durationMs / 3_600_000),
        })),
        rawUserPrompts: session.rawUserPrompts,
        rawMessages: session.messages,
      })),
  };

  // Keep printed summaries readable while the output file retains raw messages.
  const printable = {
    ...report,
    totals: {
      ...report.totals,
      toolDurationHours: round(report.totals.toolDurationHours),
      tools: report.totals.tools.slice(0, 20).map((tool) => ({
        toolName: tool.toolName,
        calls: tool.calls,
        durationHours: round(tool.durationMs / 3_600_000),
      })),
    },
    strict: {
      ...report.strict,
      toolDurationHours: round(report.strict.toolDurationHours),
      active: { ...report.strict.active, hours: round(report.strict.active.hours) },
      tools: report.strict.tools.slice(0, 20).map((tool) => ({
        toolName: tool.toolName,
        calls: tool.calls,
        durationHours: round(tool.durationMs / 3_600_000),
      })),
    },
    expanded: {
      ...report.expanded,
      toolDurationHours: round(report.expanded.toolDurationHours),
      active: { ...report.expanded.active, hours: round(report.expanded.active.hours) },
      tools: report.expanded.tools.slice(0, 20).map((tool) => ({
        toolName: tool.toolName,
        calls: tool.calls,
        durationHours: round(tool.durationMs / 3_600_000),
      })),
    },
    sessions: report.sessions.map((session) => ({
      ...session,
      rawMessages: undefined,
      rawUserPrompts: session.rawUserPrompts.map((prompt) => ({
        ...prompt,
        content: prompt.content.length > 240 ? `${prompt.content.slice(0, 240)}...` : prompt.content,
      })),
    })),
  };

  if (opts.out) {
    const outPath = resolve(opts.out);
    mkdirSync(dirname(outPath), { recursive: true });
    writeFileSync(outPath, `${JSON.stringify(report, null, 2)}\n`);
    printable.output = outPath;
  }

  if (opts.json || opts.out) {
    console.log(JSON.stringify(printable, null, 2));
  } else {
    console.log(`Codex sessions today: ${printable.totals.codexSessions}`);
    console.log(`Strict deployment: ${printable.strict.sessionCount} sessions, ${printable.strict.messageCount} messages, active ${printable.strict.active.hours}h, tools ${printable.strict.toolCalls}, tool duration ${printable.strict.toolDurationHours}h`);
    console.log(`Expanded deployment: ${printable.expanded.sessionCount} sessions, ${printable.expanded.messageCount} messages, active ${printable.expanded.active.hours}h, tools ${printable.expanded.toolCalls}, tool duration ${printable.expanded.toolDurationHours}h`);
    if (opts.out) console.log(`Output: ${opts.out}`);
  }
}

try {
  main();
} catch (error) {
  console.error(error instanceof Error ? error.message : String(error));
  process.exit(1);
}
