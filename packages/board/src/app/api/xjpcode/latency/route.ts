import { randomUUID } from 'node:crypto';
import { NextRequest, NextResponse } from 'next/server';
import { resolveXjpcodeBaseUrl, xjpcodeUrl } from '@/lib/xjpcodeProxy';

export const runtime = 'nodejs';
export const dynamic = 'force-dynamic';
export const revalidate = 0;

type LatencyPayload = {
  baseUrl?: unknown;
  model?: unknown;
  input?: unknown;
  timeoutMs?: unknown;
};

function textArg(value: unknown): string {
  return typeof value === 'string' ? value.trim() : '';
}

function timeoutArg(value: unknown): number {
  const numeric = typeof value === 'number' ? value : Number(value);
  if (!Number.isFinite(numeric)) return 60_000;
  return Math.min(Math.max(Math.round(numeric), 5_000), 120_000);
}

async function readErrorText(response: Response): Promise<string> {
  const text = await response.text().catch(() => '');
  if (!text.trim()) return '';
  try {
    const parsed = JSON.parse(text) as { error?: unknown; message?: unknown };
    if (typeof parsed.error === 'string') return parsed.error;
    if (typeof parsed.message === 'string') return parsed.message;
  } catch {
    // Fall through to raw text.
  }
  return text;
}

async function cleanupSession(baseUrl: string, sessionId: string) {
  await fetch(xjpcodeUrl(baseUrl, `/v1/sessions/${encodeURIComponent(sessionId)}`), {
    method: 'DELETE',
    cache: 'no-store',
  }).catch(() => null);
}

async function drainSse(response: Response, startedAt: number) {
  const reader = response.body?.getReader();
  if (!reader) return { firstByteMs: null, textChars: 0, sawDone: false };

  const decoder = new TextDecoder();
  let buffer = '';
  let firstByteMs: number | null = null;
  let textChars = 0;
  let sawDone = false;

  while (true) {
    const { done, value } = await reader.read();
    if (done) break;
    if (firstByteMs === null) firstByteMs = Date.now() - startedAt;
    buffer += decoder.decode(value, { stream: true });

    let index;
    while ((index = buffer.indexOf('\n')) !== -1) {
      const line = buffer.slice(0, index).trim();
      buffer = buffer.slice(index + 1);
      if (!line.startsWith('data:')) continue;

      const json = line.slice(5).trim();
      if (!json) continue;
      try {
        const frame = JSON.parse(json) as { type?: unknown; content?: unknown };
        if (frame.type === 'text' && typeof frame.content === 'string') textChars += frame.content.length;
        if (frame.type === 'done') sawDone = true;
      } catch {
        // Ignore malformed event fragments from the probe path.
      }
    }
  }

  return { firstByteMs, textChars, sawDone };
}

export async function POST(req: NextRequest) {
  let payload: LatencyPayload;
  try {
    payload = await req.json();
  } catch {
    return NextResponse.json({ ok: false, error: 'Invalid JSON body' }, { status: 400 });
  }

  let baseUrl: string;
  try {
    baseUrl = resolveXjpcodeBaseUrl(payload.baseUrl);
  } catch (err) {
    return NextResponse.json({ ok: false, error: String(err) }, { status: 400 });
  }

  const model = textArg(payload.model);
  const input = textArg(payload.input) || 'Reply with exactly OK.';
  const timeoutMs = timeoutArg(payload.timeoutMs);

  if (!model) return NextResponse.json({ ok: false, error: 'model is required' }, { status: 400 });

  const sessionId = `xjpcode-latency-${randomUUID()}`;
  const controller = new AbortController();
  const timeout = setTimeout(() => controller.abort(), timeoutMs);
  req.signal.addEventListener('abort', () => controller.abort(), { once: true });
  const startedAt = Date.now();

  try {
    const upstream = await fetch(xjpcodeUrl(baseUrl, '/v1/chat/completions'), {
      method: 'POST',
      headers: {
        accept: 'text/event-stream',
        'content-type': 'application/json',
      },
      body: JSON.stringify({
        session_id: sessionId,
        input,
        model,
      }),
      cache: 'no-store',
      signal: controller.signal,
    });

    if (!upstream.ok || !upstream.body) {
      const durationMs = Date.now() - startedAt;
      const error = await readErrorText(upstream);
      return NextResponse.json({
        ok: false,
        model,
        durationMs,
        status: upstream.status,
        error: error || `xjpcode HTTP ${upstream.status}`,
      }, { status: upstream.status || 502 });
    }

    const result = await drainSse(upstream, startedAt);
    const durationMs = Date.now() - startedAt;

    return NextResponse.json({
      ok: true,
      model,
      durationMs,
      firstByteMs: result.firstByteMs,
      textChars: result.textChars,
      done: result.sawDone,
    });
  } catch (err) {
    const durationMs = Date.now() - startedAt;
    const aborted = err instanceof Error && err.name === 'AbortError';
    return NextResponse.json({
      ok: false,
      model,
      durationMs,
      error: aborted ? `latency probe timed out after ${durationLabel(timeoutMs)}` : String(err),
    }, { status: aborted ? 504 : 502 });
  } finally {
    clearTimeout(timeout);
    await cleanupSession(baseUrl, sessionId);
  }
}

function durationLabel(durationMs: number): string {
  const seconds = durationMs / 1000;
  if (seconds < 10) return `${seconds.toFixed(1)}s`;
  return `${Math.round(seconds)}s`;
}
