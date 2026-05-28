#!/usr/bin/env node

const args = new Set(process.argv.slice(2));
const json = args.has('--json');
const allowCreate = args.has('--allow-create') || process.env.JARVIS_DISPATCH_ALLOW_CREATE === '1';
const followToFinal = args.has('--follow') || process.env.JARVIS_DISPATCH_FOLLOW === '1';
const followTaskOnly = String(process.env.JARVIS_FOLLOW_TASK_ID || '').trim();
const baseUrl = stripTrailingSlash(process.env.JARVIS_BASE_URL || 'https://jarvis.xiaojinpro.top');
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
  const timeout = setTimeout(() => controller.abort(), Number(process.env.JARVIS_SMOKE_TIMEOUT_MS || 180000));
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
    if (data.phase === 'board_tasks_created' && data.terminal_task_result === false) return 'dispatch_accepted';
    if (data.phase === 'result_pending' || data.status === 'result_pending') return 'result_pending';
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

function hasOpenAIArtifactProjection(response, expectedEvent) {
  return (response?.events || []).some((event) => {
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

function openAIContent(data) {
  return data?.choices?.[0]?.delta?.content;
}

function hasVisibleProgress(response, expectedPhase) {
  return (response?.events || []).some((event) => {
    const data = event?.data;
    if (!data || typeof data !== 'object') return false;
    if (
      data.schema === 'missiond.jarvis-progress.v1'
      && data.phase === expectedPhase
      && data.visible === true
      && data.openai_delta === true
      && data.event_bus_write_ok === true
      && data.event_bus_projection === 'frontend_event_bus'
    ) {
      return true;
    }
    const content = openAIContent(data);
    return typeof content === 'string'
      && content.includes(expectedPhase === 'plan_authoring' ? 'plan.lisp' : 'intent.lisp')
      && /正在|仍在运行|已由 Codex|失败在/.test(content);
  });
}

function hasReviewableArtifactDraft(response, expectedEvent) {
  return (response?.events || []).some((event) => {
    const data = event?.data;
    if (!data || typeof data !== 'object') return false;
    const matchesIntent = expectedEvent === 'intent_draft'
      && (inferredEventName(event) === 'intent_draft' || data.phase === 'intent_draft' || data.intent_artifact_id);
    const matchesPlan = expectedEvent === 'plan_draft'
      && (inferredEventName(event) === 'plan_draft' || data.phase === 'plan_draft' || data.plan_artifact_id);
    const expectedForm = expectedEvent === 'plan_draft' ? '(plan-draft' : '(intent-draft';
    return (matchesIntent || matchesPlan)
      && typeof data.review_text === 'string'
      && data.review_text.trim().length > 0
      && data.artifact_language === 'lisp'
      && typeof data.artifact_body === 'string'
      && data.artifact_body.includes(expectedForm)
      && (expectedEvent !== 'intent_draft'
        || (data.author === 'codex-cli-gpt-5.5-xhigh'
          && data.artifact_body.includes(':authority codex-cli-gpt-5.5-xhigh')))
      && (expectedEvent !== 'plan_draft'
        || (data.author === 'codex-cli-gpt-5.5-xhigh'
          && data.plan_author_slot_id === 'slot-codex-plan-author'
          && data.artifact_body.includes(':authority codex-cli-gpt-5.5-xhigh')
          && data.artifact_body.includes(':semantic-author')));
  });
}

function eventDataObjects(response) {
  return (response?.events || [])
    .map((event) => event?.data)
    .filter((data) => data && typeof data === 'object');
}

function findFollowTaskId(response) {
  for (const data of eventDataObjects(response)) {
    const direct = data.missiond_follow_task_id;
    if (typeof direct === 'string' && direct.trim()) return direct.trim();
    const followPayload = data.follow_payload || data.followPayload;
    if (followPayload && typeof followPayload === 'object') {
      const nested = followPayload.missiond_follow_task_id || followPayload.task_id;
      if (typeof nested === 'string' && nested.trim()) return nested.trim();
    }
    if (typeof data.task_id === 'string' && (data.phase === 'result_pending' || data.status === 'result_pending')) {
      return data.task_id.trim();
    }
  }
  return '';
}

function hasResultArtifact(response) {
  return (response?.events || []).some((event) => {
    if (event?.event === 'result_artifact') return true;
    const data = event?.data;
    return Boolean(data && typeof data === 'object' && (data.artifact_hash || data.task_result_artifact_hash));
  });
}

function hasTerminalFinal(response) {
  return (response?.events || []).some((event) => {
    if (event?.event !== 'final') return false;
    const data = event.data;
    if (!data || typeof data !== 'object') return true;
    return data.terminal_task_result !== false
      && data.phase !== 'result_pending'
      && data.status !== 'result_pending';
  });
}

function hasNonTerminalFinal(response) {
  return (response?.events || []).some((event) => {
    if (event?.event !== 'final') return false;
    const data = event.data;
    if (!data || typeof data !== 'object') return true;
    if (data.terminal_task_result === false) return true;
    if (data.phase === 'result_pending' || data.status === 'result_pending') return true;
    return false;
  });
}

async function followTaskUntilTerminal(taskId, diagnostics) {
  const maxAttempts = Number(process.env.JARVIS_FOLLOW_MAX_ATTEMPTS || 8);
  const delayMs = Number(process.env.JARVIS_FOLLOW_RETRY_MS || 5000);
  const attempts = [];
  for (let attempt = 1; attempt <= maxAttempts; attempt += 1) {
    const response = await postInteraction(buildBody({
      missiond_follow_task_id: taskId,
      missiond_follow: { task_id: taskId, stream: true },
    }));
    attempts.push(summarize(response));
    if (!response.ok) {
      if (isRetryableFollowTransport(response) && attempt < maxAttempts) {
        attempts[attempts.length - 1].retryable_transport = true;
        attempts[attempts.length - 1].body_sample = response.body_sample;
        await new Promise((resolve) => setTimeout(resolve, delayMs));
        continue;
      }
      validateHttp(`follow-${attempt}`, response, diagnostics);
      return { attempts, terminal: false };
    }
    if (hasNonTerminalFinal(response)) {
      diagnostics.push({
        code: 'FOLLOW_NON_TERMINAL_FINAL',
        message: 'follow emitted a non-terminal final; follow must return result_pending until task-result-artifact is terminal',
        events: eventNames(response),
      });
    }
    if (hasTerminalFinal(response)) {
      if (!hasResultArtifact(response)) {
        diagnostics.push({
          code: 'FOLLOW_FINAL_WITHOUT_RESULT_ARTIFACT',
          message: 'follow reached final without a result_artifact event',
          events: eventNames(response),
        });
      }
      return { attempts, terminal: true };
    }
    if (attempt < maxAttempts) {
      await new Promise((resolve) => setTimeout(resolve, delayMs));
    }
  }
  diagnostics.push({
    code: 'FOLLOW_TERMINAL_RESULT_MISSING',
    message: `follow did not reach terminal final within ${maxAttempts} attempt(s)`,
    task_id: taskId,
    attempts,
  });
  return { attempts, terminal: false };
}

function isRetryableFollowTransport(response) {
  if (!response || response.ok) return false;
  if (![502, 503, 504].includes(Number(response.status))) return false;
  const sample = String(response.body_sample || '').toLowerCase();
  return sample.includes('client_waking')
    || sample.includes('client waking')
    || sample.includes('request timeout')
    || sample.includes('retry in')
    || sample.includes('temporarily unavailable')
    || sample.includes('upstream request timeout');
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

  if (followTaskOnly) {
    const diagnostics = [];
    const follow = await followTaskUntilTerminal(followTaskOnly, diagnostics);
    return finish({
      ok: diagnostics.length === 0 && follow.terminal,
      schema: 'missiond.jarvis-intent-plan-dispatch-smoke.v1',
      mode: 'follow-only',
      task_id: followTaskOnly,
      follow,
      diagnostics,
    }, diagnostics.length === 0 && follow.terminal ? 0 : 1);
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
    if (!hasOpenAIArtifactProjection(first, 'intent_draft')) {
      diagnostics.push({
        code: 'OPENAI_ARTIFACT_PROJECTION_MISSING',
        message: 'intent_draft must also be visible as an OpenAI-compatible artifact delta',
        events: eventNames(first),
      });
    }
    if (!hasVisibleProgress(first, 'intent_authoring')) {
      diagnostics.push({
        code: 'VISIBLE_PROGRESS_MISSING',
        message: 'intent authoring must emit missiond.jarvis-progress.v1 and OpenAI-compatible progress deltas while waiting.',
        events: eventNames(first),
      });
    }
    if (!hasReviewableArtifactDraft(first, 'intent_draft')) {
      diagnostics.push({
        code: 'REVIEWABLE_ARTIFACT_BODY_MISSING',
        message: 'intent_draft must carry review_text plus Lisp artifact_body before intent confirmation.',
        events: eventNames(first),
      });
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
      if (!hasOpenAIArtifactProjection(second, 'plan_draft')) {
        diagnostics.push({
          code: 'OPENAI_ARTIFACT_PROJECTION_MISSING',
          message: 'plan_draft must also be visible as an OpenAI-compatible artifact delta',
          events: eventNames(second),
        });
      }
      if (!hasVisibleProgress(second, 'plan_authoring')) {
        diagnostics.push({
          code: 'VISIBLE_PROGRESS_MISSING',
          message: 'plan authoring must emit missiond.jarvis-progress.v1 and OpenAI-compatible progress deltas while waiting.',
          events: eventNames(second),
        });
      }
      if (!hasReviewableArtifactDraft(second, 'plan_draft')) {
        diagnostics.push({
          code: 'REVIEWABLE_ARTIFACT_BODY_MISSING',
          message: 'plan_draft must carry review_text plus Lisp artifact_body before plan confirmation.',
          events: eventNames(second),
        });
      }
    }
    planConfirm = findConfirmPayload(second);
  } else {
    diagnostics.push({ code: 'INTENT_CONFIRM_PAYLOAD_MISSING', message: 'intent phase did not return a confirm payload' });
  }

  let third = null;
  let follow = null;
  if (allowCreate && planConfirm) {
    third = await postInteraction(buildBody({ missiond_plan_confirmed: true, missiond_confirm: { confirm_payload: planConfirm } }));
    validateHttp('dispatch', third, diagnostics);
    if (third.ok) {
      for (const required of ['board_task_created']) {
        if (!hasEvent(third, required)) {
          diagnostics.push({ code: 'DISPATCH_EVENT_MISSING', message: `dispatch phase missing ${required}`, events: eventNames(third) });
        }
      }
      const thirdEvents = eventNames(third);
      if (!thirdEvents.includes('dispatch_accepted') && !thirdEvents.includes('result_pending')) {
        diagnostics.push({ code: 'DISPATCH_PENDING_EVENT_MISSING', message: 'dispatch phase must return dispatch_accepted or result_pending for non-terminal async work', events: thirdEvents });
      }
      if (hasNonTerminalFinal(third)) {
        diagnostics.push({ code: 'NON_TERMINAL_FINAL', message: 'dispatch phase emitted final before a terminal task-result-artifact; use result_pending/dispatch_accepted instead', events: thirdEvents });
      }
      if (followToFinal) {
        const followTaskId = findFollowTaskId(third);
        if (!followTaskId) {
          diagnostics.push({
            code: 'FOLLOW_TASK_ID_MISSING',
            message: 'dispatch phase did not return follow_payload.missiond_follow_task_id',
            events: thirdEvents,
          });
        } else {
          follow = await followTaskUntilTerminal(followTaskId, diagnostics);
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
      follow,
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
