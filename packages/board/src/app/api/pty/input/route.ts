import { NextRequest, NextResponse } from 'next/server';
import { callTool } from '@/lib/missiond';

const KEY_BYTES: Record<string, string> = {
  enter: '\r',
  escape: '\x1b',
  esc: '\x1b',
  tab: '\t',
  up: '\x1b[A',
  down: '\x1b[B',
  right: '\x1b[C',
  left: '\x1b[D',
  'ctrl-c': '\x03',
  'ctrl-d': '\x04',
};

export const runtime = 'nodejs';
export const dynamic = 'force-dynamic';
export const revalidate = 0;

export async function POST(req: NextRequest) {
  try {
    const body = await req.json().catch(() => ({}));
    const slotId = typeof body.slotId === 'string' ? body.slotId.trim() : '';
    const key = typeof body.key === 'string' ? body.key.trim().toLowerCase() : '';
    const text = typeof body.text === 'string' ? body.text : '';

    if (!slotId) return NextResponse.json({ error: 'Missing slotId' }, { status: 400 });

    const message = key ? KEY_BYTES[key] : text;
    if (message === undefined) {
      return NextResponse.json({ error: `Unsupported key: ${key}` }, { status: 400 });
    }
    if (!message) {
      return NextResponse.json({ error: 'Missing text or key' }, { status: 400 });
    }

    const result = await callTool('mission_pty_send', {
      slotId,
      message,
      waitForResponse: false,
    });
    return NextResponse.json({ ok: true, slotId, key: key || null, bytes: message.length, result });
  } catch (err) {
    return NextResponse.json({ error: String(err) }, { status: 502 });
  }
}
