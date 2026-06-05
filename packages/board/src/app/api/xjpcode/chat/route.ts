import { NextRequest, NextResponse } from 'next/server';
import { resolveXjpcodeBaseUrl, xjpcodeUrl } from '@/lib/xjpcodeProxy';

export const runtime = 'nodejs';
export const dynamic = 'force-dynamic';
export const revalidate = 0;

type ChatPayload = {
  baseUrl?: unknown;
  session_id?: unknown;
  sessionId?: unknown;
  input?: unknown;
  model?: unknown;
};

function textArg(value: unknown): string {
  return typeof value === 'string' ? value.trim() : '';
}

export async function POST(req: NextRequest) {
  let payload: ChatPayload;
  try {
    payload = await req.json();
  } catch {
    return NextResponse.json({ error: 'Invalid JSON body' }, { status: 400 });
  }

  let baseUrl: string;
  try {
    baseUrl = resolveXjpcodeBaseUrl(payload.baseUrl);
  } catch (err) {
    return NextResponse.json({ error: String(err) }, { status: 400 });
  }

  const sessionId = textArg(payload.session_id) || textArg(payload.sessionId);
  const input = textArg(payload.input);
  const model = textArg(payload.model);

  if (!sessionId) return NextResponse.json({ error: 'session_id is required' }, { status: 400 });
  if (!input) return NextResponse.json({ error: 'input is required' }, { status: 400 });

  let upstream: Response;
  try {
    upstream = await fetch(xjpcodeUrl(baseUrl, '/v1/chat/completions'), {
      method: 'POST',
      headers: {
        accept: 'text/event-stream',
        'content-type': 'application/json',
      },
      body: JSON.stringify({
        session_id: sessionId,
        input,
        model: model || null,
      }),
      cache: 'no-store',
      signal: req.signal,
    });
  } catch (err) {
    return NextResponse.json({
      error: String(err),
      baseUrl,
    }, { status: 502 });
  }

  if (!upstream.ok || !upstream.body) {
    const body = await upstream.text().catch(() => '');
    return NextResponse.json({
      error: body || `xjpcode HTTP ${upstream.status}`,
      baseUrl,
    }, { status: upstream.status || 502 });
  }

  return new Response(upstream.body, {
    status: 200,
    headers: {
      'cache-control': 'no-cache, no-transform',
      connection: 'keep-alive',
      'content-type': 'text/event-stream; charset=utf-8',
      'x-accel-buffering': 'no',
    },
  });
}
