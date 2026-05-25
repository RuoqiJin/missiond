#!/usr/bin/env node

const args = new Set(process.argv.slice(2));
const json = args.has('--json');
const allowCreate = args.has('--allow-create') || process.env.JARVIS_DISPATCH_ALLOW_CREATE === '1';
const baseUrl = stripTrailingSlash(process.env.JARVIS_BASE_URL || 'https://auth.xiaojinpro.com/jarvis');
const smokeSecretRef =
  process.env.MISSIOND_JARVIS_SMOKE_SECRET_REF ||
  'missiond.jarvis-smoke/INTERACTION_SERVICE_TOKEN';
let token =
  process.env.MISSIOND_JARVIS_SMOKE_TOKEN ||
  process.env.MISSIOND_INTERACTION_SERVICE_TOKEN ||
  '';
const objective =
  process.env.JARVIS_DISPATCH_OBJECTIVE ||
  '只读 smoke：请审视 Codex CLI 是否能作为普通工位，并派一个只读审查任务。不要改文件、不要提交。';

function stripTrailingSlash(value) {
  return String(value).replace(/\/+$/, '');
}

function buildBody(extra = {}) {
  return {
    model: 'missiond-jarvis',
    stream: true,
    messages: [{ role: 'user', content: objective }],
    ...extra,
  };
}

async function tokenFromSecretStore(ref) {
  const parsed = parseSecretRef(ref);
  if (!parsed) return '';
  const { spawnSync } = await import('node:child_process');
  const result = spawnSync('xjp', ['secret', 'get', '--raw', parsed.key, '--ns', parsed.namespace], {
    encoding: 'utf8',
    env: process.env,
    stdio: ['ignore', 'pipe', 'pipe'],
  });
  return result.status === 0 ? String(result.stdout || '').trim() : '';
}

function parseSecretRef(ref) {
  const parts = String(ref || '').split('/').filter(Boolean);
  if (parts.length < 2) return null;
  const key = parts.pop();
  return { namespace: parts.join('/'), key };
}

async function postInteraction(body) {
  const controller = new AbortController();
  const timeout = setTimeout(() => controller.abort(), Number(process.env.JARVIS_SMOKE_TIMEOUT_MS || 60000));
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
      return {
        ok: false,
        status: response.status,
        content_type: contentType,
        events: [],
        body_sample: (await response.text()).slice(0, 600),
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

function eventNames(response) {
  return response.events.map((event) => inferredEventName(event));
}

function inferredEventName(item) {
  if (!item) return 'unknown';
  if (item.event && item.event !== 'message') return item.event;
  const data = item.data;
  if (data && typeof data === 'object') {
    if (data.schema === 'missiond.interaction-envelope.v1') return 'received';
    if (data.authenticated === true) return 'authenticated';
    if (data.permission_context) return 'permission_resolved';
    if (data.phase === 'grounding' || data.grounding_context_id) return 'grounding';
    if (data.intent_artifact_id && data.objective) return 'intent_draft';
    if (data.plan_artifact_id && data.steps) return 'plan_draft';
    if (data.confirm_payload) return 'confirm_required';
    if (data.board_task_id || data.board_task_ids) return 'board_task_created';
    if (data.status === 'workers_running' || data.phase === 'workers_running') return 'worker_status';
  }
  return item.event || 'message';
}

function findConfirmPayload(response) {
  for (const event of response.events) {
    const data = event?.data;
    if (data && typeof data === 'object') {
      const payload = data.confirm_payload || data.confirmPayload;
      if (payload && typeof payload === 'object') return payload;
      if (typeof payload === 'string') {
        try {
          return JSON.parse(payload);
        } catch {
          return { missiond_confirm_payload_text: payload };
        }
      }
    }
    const direct = event?.confirm_payload || event?.confirmPayload;
    if (direct && typeof direct === 'object') return direct;
  }
  return null;
}

function hasEvent(response, name) {
  return eventNames(response).includes(name);
}

async function main() {
  if (!token && process.env.MISSIOND_JARVIS_SMOKE_SECRET_REF !== '0') {
    token = await tokenFromSecretStore(smokeSecretRef);
  }
  if (!token) {
    return finish({
      ok: false,
      schema: 'missiond.jarvis-intent-plan-dispatch-smoke.v1',
      diagnostics: [{
        code: 'INTERACTION_AUTH_REQUIRED',
        message: 'Set MISSIOND_JARVIS_SMOKE_TOKEN or provision secret-store ref missiond.jarvis-smoke/INTERACTION_SERVICE_TOKEN; token values are never printed.',
        secret_ref: smokeSecretRef,
      }],
    }, 2);
  }

  const diagnostics = [];
  const first = await postInteraction(buildBody());
  validateHttp('intent', first, diagnostics);
  if (first.ok) {
    for (const required of ['received', 'authenticated', 'grounding', 'intent_draft', 'confirm_required']) {
      if (!hasEvent(first, required)) {
        diagnostics.push({ code: 'INTENT_EVENT_MISSING', message: `intent phase missing ${required}`, events: eventNames(first) });
      }
    }
    if (hasEvent(first, 'board_task_created') || hasEvent(first, 'worker_dispatched')) {
      diagnostics.push({ code: 'CONFIRMATION_BYPASS', message: 'initial request created/dispatched work before confirmation' });
    }
  }
  const intentConfirm = findConfirmPayload(first);

  let second = null;
  let planConfirm = null;
  if (intentConfirm) {
    second = await postInteraction(buildBody({ missiond_intent_confirmed: true, missiond_confirm: { confirm_payload: intentConfirm } }));
    validateHttp('plan', second, diagnostics);
    if (second.ok) {
      for (const required of ['plan_draft', 'confirm_required']) {
        if (!hasEvent(second, required)) {
          diagnostics.push({ code: 'PLAN_EVENT_MISSING', message: `plan phase missing ${required}`, events: eventNames(second) });
        }
      }
      if (hasEvent(second, 'board_task_created') || hasEvent(second, 'worker_dispatched')) {
        diagnostics.push({ code: 'PLAN_CONFIRMATION_BYPASS', message: 'plan draft phase created/dispatched work before plan confirmation' });
      }
    }
    planConfirm = findConfirmPayload(second);
  } else {
    diagnostics.push({ code: 'INTENT_CONFIRM_PAYLOAD_MISSING', message: 'intent phase did not return a confirm payload' });
  }

  let third = null;
  if (allowCreate && planConfirm) {
    third = await postInteraction(buildBody({ missiond_plan_confirmed: true, missiond_confirm: { confirm_payload: planConfirm } }));
    validateHttp('dispatch', third, diagnostics);
    if (third.ok) {
      for (const required of ['board_task_created', 'final']) {
        if (!hasEvent(third, required)) {
          diagnostics.push({ code: 'DISPATCH_EVENT_MISSING', message: `dispatch phase missing ${required}`, events: eventNames(third) });
        }
      }
    }
  }

  finish({
    ok: diagnostics.length === 0,
    schema: 'missiond.jarvis-intent-plan-dispatch-smoke.v1',
    base_url: baseUrl,
    mode: allowCreate ? 'confirm-plan-and-create-task' : 'confirm-through-plan-only',
    phases: {
      intent: summarize(first),
      plan: summarize(second),
      dispatch: summarize(third),
    },
    diagnostics,
  }, diagnostics.length === 0 ? 0 : 1);
}

function validateHttp(phase, response, diagnostics) {
  if (!response?.ok) {
    diagnostics.push({
      code: response?.status === 401 ? 'INTERACTION_AUTH_INVALID' : 'JARVIS_INTERACTION_HTTP_FAILED',
      message: `${phase} phase returned HTTP ${response?.status ?? 'missing'}`,
      body_sample: response?.body_sample,
    });
  }
}

function summarize(response) {
  if (!response) return null;
  return {
    ok: response.ok,
    http_status: response.status,
    content_type: response.content_type,
    events: eventNames(response),
  };
}

function finish(result, code) {
  if (json) {
    console.log(JSON.stringify(result, null, 2));
  } else if (result.ok) {
    console.log(`Jarvis intent/plan dispatch smoke OK (${result.mode})`);
  } else {
    console.error(JSON.stringify(result, null, 2));
  }
  process.exit(code);
}

main().catch((error) => finish({
  ok: false,
  schema: 'missiond.jarvis-intent-plan-dispatch-smoke.v1',
  diagnostics: [{ code: 'JARVIS_DISPATCH_SMOKE_EXCEPTION', message: error.message }],
}, 1));
