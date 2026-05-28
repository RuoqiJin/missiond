#!/usr/bin/env node

const args = new Set(process.argv.slice(2));
const json = args.has('--json');
const baseUrl = stripTrailingSlash(
  process.env.JARVIS_BASE_URL || 'https://auth.xiaojinpro.com/jarvis',
);
const smokeSecretRef =
  process.env.MISSIOND_JARVIS_SMOKE_SECRET_REF ||
  'missiond.jarvis-smoke/INTERACTION_SERVICE_TOKEN';
let token =
  process.env.MISSIOND_JARVIS_SMOKE_TOKEN ||
  process.env.MISSIOND_INTERACTION_SERVICE_TOKEN ||
  '';
const objective =
  process.env.JARVIS_SMOKE_OBJECTIVE ||
  '只读 smoke：请确认 MissionD Jarvis intent/plan gate 是否会先生成 intent draft，不要改文件、不要创建部署。';

function stripTrailingSlash(value) {
  return String(value).replace(/\/+$/, '');
}

function buildRequestBody(extra = {}) {
  return {
    model: 'missiond-jarvis',
    stream: true,
    messages: [
      {
        role: 'user',
        content: objective,
      },
    ],
    ...extra,
  };
}

async function readSse(response) {
  const reader = response.body.getReader();
  const decoder = new TextDecoder();
  let buffer = '';
  const events = [];
  while (true) {
    const { done, value } = await reader.read();
    if (done) break;
    buffer += decoder.decode(value, { stream: true });
    let boundary;
    while ((boundary = buffer.indexOf('\n\n')) >= 0) {
      const raw = buffer.slice(0, boundary);
      buffer = buffer.slice(boundary + 2);
      const parsed = parseSseEvent(raw);
      if (parsed) events.push(parsed);
    }
  }
  if (buffer.trim()) {
    const parsed = parseSseEvent(buffer);
    if (parsed) events.push(parsed);
  }
  return events;
}

function parseSseEvent(raw) {
  const lines = raw.split(/\r?\n/);
  let event = 'message';
  const dataLines = [];
  for (const line of lines) {
    if (line.startsWith('event:')) event = line.slice('event:'.length).trim();
    if (line.startsWith('data:')) dataLines.push(line.slice('data:'.length).trimStart());
  }
  if (dataLines.length === 0) return null;
  const dataText = dataLines.join('\n');
  let data = dataText;
  if (dataText !== '[DONE]') {
    try {
      data = JSON.parse(dataText);
    } catch {
      data = dataText;
    }
  }
  return { event, data };
}

function eventNames(events) {
  return events.map((event) => event.event);
}

function includesAny(events, candidates) {
  const names = new Set(eventNames(events));
  return candidates.some((candidate) => names.has(candidate));
}

function hasOpenAIArtifactProjection(events, expectedEvent) {
  return events.some((event) => {
    const data = event?.data;
    if (!data || typeof data !== 'object') return false;
    const projection = data.missiond_projection;
    const content = data.choices?.[0]?.delta?.content;
    return projection?.schema === 'missiond.openai-artifact-projection.v1'
      && projection?.event === expectedEvent
      && typeof content === 'string'
      && content.includes('artifact_id:');
  });
}

function hasReviewableArtifactDraft(events, expectedEvent) {
  return events.some((event) => {
    const data = event?.data;
    if (!data || typeof data !== 'object') return false;
    const matchesIntent = expectedEvent === 'intent_draft'
      && (event.event === 'intent_draft' || data.phase === 'intent_draft' || data.intent_artifact_id);
    const matchesPlan = expectedEvent === 'plan_draft'
      && (event.event === 'plan_draft' || data.phase === 'plan_draft' || data.plan_artifact_id);
    const expectedForm = expectedEvent === 'plan_draft' ? '(plan-draft' : '(intent-draft';
    return (matchesIntent || matchesPlan)
      && typeof data.review_text === 'string'
      && data.review_text.trim().length > 0
      && data.artifact_language === 'lisp'
      && typeof data.artifact_body === 'string'
      && data.artifact_body.includes(expectedForm);
  });
}

function parseSecretRef(ref) {
  const text = String(ref || '').trim();
  if (!text || !text.includes('/')) return null;
  const parts = text.split('/').filter(Boolean);
  if (parts.length < 2) return null;
  const key = parts.pop();
  return { namespace: parts.join('/'), key };
}

async function tokenFromSecretStore(ref) {
  const parsed = parseSecretRef(ref);
  if (!parsed) return '';
  const { spawnSync } = await import('node:child_process');
  const result = spawnSync(
    'xjp',
    ['secret', 'get', '--raw', parsed.key, '--ns', parsed.namespace],
    {
      encoding: 'utf8',
      env: process.env,
      stdio: ['ignore', 'pipe', 'pipe'],
    },
  );
  if (result.status !== 0) return '';
  return String(result.stdout || '').trim();
}

async function postInteraction(body) {
  const controller = new AbortController();
  const timeout = setTimeout(() => controller.abort(), Number(process.env.JARVIS_SMOKE_TIMEOUT_MS || 30000));
  try {
    const response = await fetch(`${baseUrl}/v1/chat/completions`, {
      method: 'POST',
      headers: {
        authorization: `Bearer ${token}`,
        'content-type': 'application/json',
        accept: 'text/event-stream',
      },
      body: JSON.stringify(body),
      signal: controller.signal,
    });
    const contentType = response.headers.get('content-type') || '';
    if (!response.ok) {
      const text = await response.text();
      return {
        ok: false,
        status: response.status,
        content_type: contentType,
        events: [],
        body_sample: text.slice(0, 500),
      };
    }
    const events = contentType.includes('text/event-stream')
      ? await readSse(response)
      : [{ event: 'http_body', data: await response.text() }];
    return { ok: true, status: response.status, content_type: contentType, events };
  } finally {
    clearTimeout(timeout);
  }
}

async function main() {
  if (!token && process.env.MISSIOND_JARVIS_SMOKE_SECRET_REF !== '0') {
    token = await tokenFromSecretStore(smokeSecretRef);
  }
  if (!token) {
    const result = {
      ok: false,
      schema: 'missiond.jarvis-interaction-smoke.v1',
      diagnostics: [
        {
          code: 'INTERACTION_AUTH_REQUIRED',
          message: 'Set MISSIOND_JARVIS_SMOKE_TOKEN or MISSIOND_INTERACTION_SERVICE_TOKEN, or provision secret-store ref missiond.jarvis-smoke/INTERACTION_SERVICE_TOKEN; token values are never printed.',
          secret_ref: smokeSecretRef,
        },
      ],
    };
    console.error(JSON.stringify(result, null, 2));
    process.exit(2);
  }

  const first = await postInteraction(buildRequestBody());
  const diagnostics = [];
  if (!first.ok) {
    diagnostics.push({
      code: first.status === 401 ? 'INTERACTION_AUTH_INVALID' : 'JARVIS_INTERACTION_HTTP_FAILED',
      message: `Jarvis interaction returned HTTP ${first.status}`,
      body_sample: first.body_sample,
    });
  } else {
    const names = eventNames(first.events);
    for (const required of ['received', 'authenticated', 'grounding', 'intent_draft', 'confirm_required']) {
      if (!names.includes(required)) {
        diagnostics.push({
          code: 'JARVIS_INTERACTION_EVENT_MISSING',
          message: `Missing expected SSE event ${required}`,
          events: names,
        });
      }
    }
    if (includesAny(first.events, ['board_task_created', 'worker_dispatched'])) {
      diagnostics.push({
        code: 'JARVIS_CONFIRMATION_BYPASS',
        message: 'Initial broad request created or dispatched work before intent/plan confirmation.',
      });
    }
    if (!hasOpenAIArtifactProjection(first.events, 'intent_draft')) {
      diagnostics.push({
        code: 'OPENAI_ARTIFACT_PROJECTION_MISSING',
        message: 'Initial broad request must mirror intent_draft as an OpenAI-compatible artifact delta for legacy iOS/chat clients.',
        events: names,
      });
    }
    if (!hasReviewableArtifactDraft(first.events, 'intent_draft')) {
      diagnostics.push({
        code: 'REVIEWABLE_ARTIFACT_BODY_MISSING',
        message: 'intent_draft must carry review_text plus Lisp artifact_body before the user can confirm intent.',
        events: names,
      });
    }
  }

  const result = {
    ok: diagnostics.length === 0,
    schema: 'missiond.jarvis-interaction-smoke.v1',
    base_url: baseUrl,
    http_status: first.status,
    content_type: first.content_type,
    events: eventNames(first.events),
    diagnostics,
  };

  if (json) {
    console.log(JSON.stringify(result, null, 2));
  } else if (result.ok) {
    console.log(`Jarvis interaction smoke OK: ${result.events.join(' -> ')}`);
  } else {
    console.error(JSON.stringify(result, null, 2));
  }
  process.exit(result.ok ? 0 : 1);
}

main().catch((error) => {
  const result = {
    ok: false,
    schema: 'missiond.jarvis-interaction-smoke.v1',
    diagnostics: [{ code: 'JARVIS_INTERACTION_SMOKE_EXCEPTION', message: error.message }],
  };
  console.error(json ? JSON.stringify(result, null, 2) : error.stack || error.message);
  process.exit(1);
});
