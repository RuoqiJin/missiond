import { NextRequest, NextResponse } from 'next/server';
import { resolveXjpcodeBaseUrl, xjpcodeUrl } from '@/lib/xjpcodeProxy';

export const runtime = 'nodejs';
export const dynamic = 'force-dynamic';
export const revalidate = 0;

type SessionPayload = {
  baseUrl?: unknown;
  session_id?: unknown;
  sessionId?: unknown;
};

function textArg(value: unknown): string {
  return typeof value === 'string' ? value.trim() : '';
}

export async function DELETE(req: NextRequest) {
  let payload: SessionPayload = {};
  try {
    payload = await req.json();
  } catch {
    payload = {};
  }

  let baseUrl: string;
  try {
    baseUrl = resolveXjpcodeBaseUrl(
      payload.baseUrl ?? req.nextUrl.searchParams.get('baseUrl'),
    );
  } catch (err) {
    return NextResponse.json({ ok: false, error: String(err) }, { status: 400 });
  }

  const sessionId = textArg(payload.session_id)
    || textArg(payload.sessionId)
    || textArg(req.nextUrl.searchParams.get('session_id'));

  if (!sessionId) {
    return NextResponse.json({ ok: false, error: 'session_id is required' }, { status: 400 });
  }

  try {
    const response = await fetch(xjpcodeUrl(baseUrl, `/v1/sessions/${encodeURIComponent(sessionId)}`), {
      method: 'DELETE',
      cache: 'no-store',
    });
    if (!response.ok && response.status !== 404) {
      const error = await response.text().catch(() => '');
      return NextResponse.json({ ok: false, error: error || `xjpcode HTTP ${response.status}` }, { status: response.status });
    }
    return NextResponse.json({ ok: true, sessionId, baseUrl });
  } catch (err) {
    return NextResponse.json({ ok: false, error: String(err), baseUrl }, { status: 502 });
  }
}
