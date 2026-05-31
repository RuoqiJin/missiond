import { NextRequest, NextResponse } from 'next/server';

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
  // AGY enables the kitty keyboard protocol, so a human Ctrl-D arrives as
  // CSI 100;5u rather than a plain EOT byte while the AGY TUI is active.
  'ctrl-d': '\x1b[100;5u',
  eof: '\x04',
};

export const runtime = 'nodejs';
export const dynamic = 'force-dynamic';
export const revalidate = 0;

function wsUrl(slotId: string) {
  const host = process.env.MISSION_WS_HOST || process.env.NEXT_PUBLIC_WS_HOST || '127.0.0.1';
  const port = process.env.MISSION_WS_PORT || process.env.NEXT_PUBLIC_WS_PORT || '9120';
  return `ws://${host}:${port}/pty/${encodeURIComponent(slotId)}`;
}

function sendRaw(slotId: string, data: string) {
  return new Promise<void>((resolve, reject) => {
    let settled = false;
    let timer: ReturnType<typeof setTimeout>;
    let ws: WebSocket;
    const finish = (err?: unknown) => {
      if (settled) return;
      settled = true;
      clearTimeout(timer);
      try {
        ws.close();
      } catch {
        // ignore close races
      }
      if (err) reject(err);
      else resolve();
    };
    timer = setTimeout(() => finish(new Error('PTY raw input timed out')), 5000);
    ws = new WebSocket(wsUrl(slotId));

    ws.addEventListener('open', () => {
      ws.send(JSON.stringify({ type: 'input', data }));
      setTimeout(() => finish(), 120);
    });
    ws.addEventListener('error', () => finish(new Error('PTY raw websocket error')));
  });
}

export async function POST(req: NextRequest) {
  try {
    const body = await req.json().catch(() => ({}));
    const slotId = typeof body.slotId === 'string' ? body.slotId.trim() : '';
    const key = typeof body.key === 'string' ? body.key.trim().toLowerCase() : '';
    const text = typeof body.text === 'string' ? body.text : '';

    if (!slotId) return NextResponse.json({ error: 'Missing slotId' }, { status: 400 });
    const data = key ? KEY_BYTES[key] : text;
    if (data === undefined) {
      return NextResponse.json({ error: `Unsupported key: ${key}` }, { status: 400 });
    }
    if (!data) return NextResponse.json({ error: 'Missing text or key' }, { status: 400 });

    await sendRaw(slotId, data);
    return NextResponse.json({ ok: true, slotId, key: key || null, bytes: data.length });
  } catch (err) {
    return NextResponse.json({ error: String(err) }, { status: 502 });
  }
}
